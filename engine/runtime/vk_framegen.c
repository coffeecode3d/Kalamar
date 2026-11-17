// Frame generation on the present (TOTK_FRAME_GEN, TOTK_FRAME_GEN_MODE): more pictures on the screen than the game
// draws. The game keeps its own frame rate and its own logic (Splatoon 3 steps its world one tick per frame, so a
// higher frame limit would run it faster); between two of its frames, or after the latest one, this shows pictures
// made from the game's frames and the motion between them. Nothing of the game's state changes, only what the screen
// shows, so it plays the same online.
//
// It works on pictures, the way a video interpolator does, and needs nothing of the game's depth, camera or motion
// vectors. The HUD must not move with the scene, so the scene is taken as it was before the HUD was drawn over it
// (vk_framegen_pass): Splatoon 3 draws its 3D scene into a target with the scene's depth attached (the tonemap pass,
// seen in a frame dump of the test range), then its HUD into the same target in passes without depth, and a last pass
// copies that target into the presented window texture as it is. The tonemap is the first draw of that pass; the
// reticle and the shot effects are drawn after it in the same pass, so the copy is made right after the tonemap: the
// pass is ended there, the target copied and the pass begun again (vk_framegen_draw). The first frame, or one where
// that pass is not found again, is copied just before the first HUD pass instead, reticle included. The scenes of the
// game frames are what moves, and where a game frame's picture differs from its scene (the HUD, the reticle), the
// picture is shown as it is (shaders/fg_synth.comp). A frame without that copy (another way of drawing, a chunked
// replay, TOTK_FRAME_GEN_HUD=0) is moved as one picture, HUD included. Each copy is matched to the
// window texture its frame is then copied into, the image the present shows, and not to a count of presents: the game
// records a frame's passes while the previous one is still being presented. fg_ingest counts the pixels where a
// picture differs from its scene; a copy matched to the wrong frame differs almost everywhere, and the synth then shows
// the game's picture as it is instead of pasting a wrong HUD (more than HUD_LIMIT of the pixels; the HUD itself is a
// few percent). The log line every 5 s gives the share of frames that had a scene and the HUD's share of the picture.
// The picture is an sRGB target stored without its decode (vk_texture.c), so its texels are sRGB-encoded, while the
// scene target holds linear values: fg_ingest stores the scene sRGB-encoded when that matches the picture better, which
// it counts for both encodings in every frame (G.encode follows the counts read).
//
// Per game frame (vk_framegen_ingest, on the present thread in place of the blit, vk_backend.c present_main):
//   fg_ingest  the crop of the game's picture into its history image (RGBA16F), of its scene into its scene image, and
//              the scene's quarter-size luma
//   fg_down    that luma at a sixteenth of the size
//   fg_flow    a coarse then a fine block-matching search between the two latest frames: one vector per 16x16 pixels
// Per shown picture (vk_framegen_present):
//   fg_synth   the picture at game time tau from the two latest frames and the motion field (shaders/fg_synth.comp)
//   then the present path of the game's own frames (gfx_present.c vk_present_generated): colour adjustment, HDR,
//   FSR 1 or the blit, the in-game menu. FSR 4 / 3.1 is left out while it is on: it is temporal and expects one
//   picture per game frame with the scene's depth.
//
// Two modes. interp (the default) shows the newer frame one game period late and the pictures between the two latest
// frames before it: smooth and right, for up to one game period of extra delay (13 ms at 60 fps with 5 pictures per
// frame). extrap shows each game frame as soon as it is due and then pictures pushed further along the latest motion:
// no extra delay, but wrong where the motion changes (a turn that starts or stops overshoots for a few milliseconds)
// and at edges where something is uncovered.
//
// Three game frames are kept, each with its picture, its luma and the motion field from the frame before it: the next
// game frame is taken as soon as it comes (vk_backend.c poll_next), while the pictures between the two before it are
// still being shown, and it never overwrites those two.
//
// Threads: the present thread alone calls ingest and present; release runs at device teardown, the device idle.
// Every pass leaves its images in GENERAL. Each pass's first barrier has the whole queue before it as its first scope,
// so an image is rewritten only after the earlier submissions that read it (a blit of the previous picture, a synth
// of the previous frame) are done.
#include "vk.h"

#include <string.h>

#include "cfg.h"
#include "fg_down_spv.h"
#include "fg_flow_spv.h"
#include "fg_ingest_spv.h"
#include "fg_synth_spv.h"
#include "gfx.h"
#include "host.h"
#include "vk_exec.h"

#define RING 4       // command buffers in flight; each waits for its fence before it records again
#define KEEP 3       // game frames kept
#define OCCLUSION 0.06f
#define FG_MAX 16    // pictures per game frame at most: a 960 Hz monitor at 60 fps

typedef struct {
    VkDescriptorSetLayout dsl;
    VkPipelineLayout layout;
    VkPipeline pipe;
} Pass;

typedef struct {
    VkImage img;
    VmaAllocation alloc;
    VkImageView view;
} Img;

static struct {
    int tried, ok;
    PFN_vkCmdPushDescriptorSetKHR push;
    Pass ingest, down, search, synth;
    VkSampler nearest, linear;
    VkCommandPool pool;
    VkCommandBuffer cb[RING];
    VkFence fence[RING];
    VkImageView src_view[RING];   // the game image's view each ring slot's last ingest read
    unsigned next;                // ring slot of the next submission
    // at the crop size w x h, per kept frame: its picture, quarter and sixteenth luma, and the motion field from the
    // frame before it; the coarse field (scratch of each ingest) and the generated picture
    uint32_t w, h, qw, qh, gw, gh;
    Img hist[KEEP], scene[KEEP], lq[KEEP], ls[KEEP], flow[KEEP], flow0, out;
    int newest;                   // the newest frame's index, -1 none since the images were made
    int prev[KEEP];               // the index of the frame before each, -1 none: no motion field
    int has_scene[KEEP];          // the scene image is the scene before the HUD (else the picture again)
    VkBuffer stats;               // per kept frame, two counters: the pixels where its picture differs from its scene
                                  // as it is and sRGB-encoded (fg_ingest)
    VmaAllocation stats_alloc;
    uint32_t *stats_map;
    int stats_pending[KEEP];      // the counter holds a frame's count the log has not read yet
    int encode, enc[KEEP];        // the scene is stored sRGB-encoded: from now on, and in each kept frame
    uint64_t log_frames, log_scenes, log_read, log_mismatch;   // since the last log line: frames, with a scene,
                                                               // counts read, counts not matching
    double log_hud;                                  // and the sum of their HUD's share of the picture
    unsigned seen;                // cfg_version the settings below were read at
    int factor, extrap;           // TOTK_FRAME_GEN (0: display), TOTK_FRAME_GEN_MODE = extrap
    int by_limit, cap_logged;     // the factor comes from the frame limit; its cap by the monitor was logged
    int last_k;                   // pictures per game frame of the latest game frame shown
} G = {.seen = ~0u, .newest = -1, .prev = {-1, -1, -1}, .encode = 1};

// TOTK_FRAME_GEN: 0 for display, else 1 (off) to FG_MAX. Any thread.
// limit (the default): the frame limit decides. A game whose world steps one tick a frame cannot draw more than 60
// frames a second (no real-time step, frame_rate.c), so a limit above 60 is reached with generated pictures: the limit
// over 60, rounded down (120: 2, 180: 3, 240: 4; 90 and 144 give 1 and 2), and the monitor's rate for display. With
// the real-time step the game draws the frames itself and limit means off.
static int setting_factor(void) {
    const char *e = cfg_value("TOTK_FRAME_GEN");
    if (e && !strcmp(e, "display")) return 0;
    if (!e || !*e || !strcmp(e, "limit")) {
        if (frame_step_realtime()) return 1;
        int hz = frame_limit_hz();
        return hz == 0 ? 0 : hz < 120 ? 1 : hz / 60 > FG_MAX ? FG_MAX : hz / 60;
    }
    int f = cfg_int("TOTK_FRAME_GEN");
    return f < 1 ? 1 : f > FG_MAX ? FG_MAX : f;
}

// ---- the scene before the HUD (the replay's thread)
// vk_framegen_pass at every render pass begin:
// - a pass with a depth target arms its first colour target, the scene's; one after a copy whose frame reached no
//   window texture drops that copy (a new frame's scene begins);
// - a pass with a depth target into the target an earlier frame's copy came from waits for its second draw
//   (vk_framegen_draw): the pass is ended, the target copied into a slot, and the pass begun again;
// - otherwise the first pass after it that draws into that same target without depth starts the HUD: the target is
//   copied into a slot first;
// - the first pass after the copy that draws into another target of the same size without depth is the copy into the
//   window texture: the slot is tagged with that image, which vk_framegen_ingest looks for (the presented image).
// Four slots: the replay is at most a few frames ahead of the present thread, which takes a frame's slot when it takes
// the frame (vk_backend.c, one present waiting at most). A slot's tag is cleared before it is rewritten and when it is
// taken.
#define HUD_SLOTS 4
#define HUD_LIMIT 0.15f   // share of the pixels above which a picture and its scene are taken as not matching
typedef struct {
    Img img;
    VkFormat fmt;
    uint32_t w, h;
    uint64_t dest;   // the window texture (VkImage) its frame is copied into; 0 none (atomic, both threads)
    uint64_t seq;    // the copy's number: the newest of two copies tagged with one image wins (atomic)
} HudSlot;
static struct {
    unsigned seen;
    int on;                          // frame generation and TOTK_FRAME_GEN_HUD are on: copies are made
    int armed, captured, logged, chunked_logged;
    VkImage target;                  // the colour target of this frame's latest pass with depth
    uint32_t level, layer;
    VkImage split_img;               // the target the last copy came from: its pass with depth is split after one draw
    int split_pending, pass_draws, in_split, split_logged;
    unsigned next, cur;              // the slot the next copy goes into, the slot of the copy not tagged yet
    uint64_t seq;
    HudSlot slot[HUD_SLOTS];
    Img old[8];                      // slot images of an earlier size or format, freed at release
    int nold;
} H = {.seen = ~0u};

static int make_img_usage(VkCtx *c, Img *m, VkFormat fmt, uint32_t w, uint32_t h, VkImageUsageFlags usage);
static void barrier(VkCommandBuffer cb, VkImage img, VkImageLayout from, VkPipelineStageFlags2 ss, VkAccessFlags2 sa,
                    VkPipelineStageFlags2 ds, VkAccessFlags2 da);

static int ensure_slot(VkCtx *c, HudSlot *s, VkFormat fmt, uint32_t w, uint32_t h) {
    if (s->img.img && s->fmt == fmt && s->w == w && s->h == h) return 1;
    if (s->img.img) {   // in use by submissions not done yet, maybe: kept until release
        if (H.nold < (int)(sizeof H.old / sizeof *H.old)) H.old[H.nold++] = s->img;
        memset(&s->img, 0, sizeof s->img);
    }
    if (!make_img_usage(c, &s->img, fmt, w, h, VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_SAMPLED_BIT)) {
        memset(&s->img, 0, sizeof s->img);
        return 0;
    }
    s->fmt = fmt, s->w = w, s->h = h;
    return 1;
}

// The copy of target t into the next slot, outside any pass
static void capture(VkExec *ex, const VkExecTarget *t) {
    VkCtx *c = vk_ctx();
    H.cur = H.next++ % HUD_SLOTS;
    HudSlot *s = &H.slot[H.cur];
    __atomic_store_n(&s->dest, 0, __ATOMIC_RELEASE);
    if (!ensure_slot(c, s, t->format, t->width, t->height)) return;
    vk_exec_barrier(ex);   // the scene's writes are done (and the pass ended)
    barrier(ex->cb, s->img.img, VK_IMAGE_LAYOUT_UNDEFINED, VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT, 0,
            VK_PIPELINE_STAGE_2_TRANSFER_BIT, VK_ACCESS_2_TRANSFER_WRITE_BIT);
    VkImageCopy cp = {.srcSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, t->level, t->base_layer, 1},
                      .dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1}, .extent = {t->width, t->height, 1}};
    vkCmdCopyImage(ex->cb, t->image, VK_IMAGE_LAYOUT_GENERAL, s->img.img, VK_IMAGE_LAYOUT_GENERAL, 1, &cp);
    ex->recorded += 2;
    vk_exec_barrier(ex);   // the draws after it wait for the copy's read; the copy is visible to later submissions
    H.captured = 1;
    H.split_img = t->image;
    if (!H.logged++)
        rt_log("vk: frame generation: the scene before the HUD is the %ux%u target of format %d", t->width, t->height,
               (int)t->format);
}

void vk_framegen_pass(VkExec *ex) {
    if (H.in_split) return;   // the pass vk_framegen_draw begins again
    unsigned v = cfg_version();
    if (v != H.seen) H.seen = v, H.on = setting_factor() != 1 && cfg_bool("TOTK_FRAME_GEN_HUD");
    H.split_pending = 0;
    if (!H.on || ex->pass_ncolor < 1) return;
    if (ex->chunked) {   // the passes are recorded out of order by several threads: no copy, the picture moves as one
        if (!H.chunked_logged++) rt_log("vk: frame generation: chunked replay, the HUD moves with the scene");
        return;
    }
    const VkExecTarget *t = &ex->color[0];
    if (!t->image || t->is3d || t->flat || t->blocks || t->width < 640 || t->height < 360) return;
    if (ex->depth.image) {
        H.captured = 0;
        H.armed = 1, H.target = t->image, H.level = t->level, H.layer = t->base_layer;
        if (t->image == H.split_img) H.split_pending = 1, H.pass_draws = 0;
        return;
    }
    HudSlot *s = &H.slot[H.cur];
    if (H.captured) {
        if (t->image != H.target && t->width == s->w && t->height == s->h) {
            __atomic_store_n(&s->seq, ++H.seq, __ATOMIC_RELAXED);
            __atomic_store_n(&s->dest, (uint64_t)(uintptr_t)t->image, __ATOMIC_RELEASE);
            H.captured = H.armed = 0;
        }
        return;
    }
    if (!H.armed || t->image != H.target || t->level != H.level || t->base_layer != H.layer) return;
    capture(ex, t);   // no split this frame: the reticle is in the copy
}

void vk_framegen_draw(VkExec *ex) {
    if (!H.split_pending || H.captured || ex->chunked || !ex->in_pass) return;
    const VkExecTarget *t = &ex->color[0];
    if (t->image != H.split_img) {
        H.split_pending = 0;
        return;
    }
    if (++H.pass_draws < 2) return;   // the first draw is the tonemap
    H.split_pending = 0;
    H.in_split = 1;
    vk_exec_pass_end(ex);
    capture(ex, t);
    vk_exec_pass_begin(ex);   // the rest of the pass (reticle, shot effects) draws over the target as before
    H.in_split = 0;
    if (!H.split_logged++) rt_log("vk: frame generation: the scene is copied after the first draw of its pass");
}

// The present thread: the newest copy tagged with the presented image, its tag cleared; NULL when there is none
static HudSlot *hud_slot(VkImage presented) {
    HudSlot *best = NULL;
    uint64_t want = (uint64_t)(uintptr_t)presented, best_seq = 0;
    for (int i = 0; i < HUD_SLOTS; i++) {
        HudSlot *s = &H.slot[i];
        if (__atomic_load_n(&s->dest, __ATOMIC_ACQUIRE) != want || !s->img.img) continue;
        uint64_t q = __atomic_load_n(&s->seq, __ATOMIC_RELAXED);
        if (!best || q > best_seq) best = s, best_seq = q;
    }
    if (best) {
        uint64_t expect = want;
        if (!__atomic_compare_exchange_n(&best->dest, &expect, 0, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) return NULL;
    }
    return best;
}

static void read_settings(void) {
    unsigned v = cfg_version();
    if (v == G.seen) return;
    G.seen = v;
    G.factor = setting_factor();
    const char *f = cfg_value("TOTK_FRAME_GEN");
    G.by_limit = !f || !*f || !strcmp(f, "limit");
    const char *m = cfg_value("TOTK_FRAME_GEN_MODE");
    G.extrap = m && !strcmp(m, "extrap");
}

// The pictures per game frame for a game period of period_ns: the setting, or for display the monitor's refresh
// rate over the game's frame rate, rounded down (a 540 Hz monitor at 60 fps: 9; 144 Hz: 2), 1 when the rate is not
// known yet.
// A factor that comes from the frame limit never exceeds what the monitor shows: a limit of 120 on a 60 Hz monitor
// would make a picture per game frame that no refresh displays, and its cost slows a loaded game. A number given in
// TOTK_FRAME_GEN is kept as it is. Not capped while the monitor's rate is unknown.
static int cap_to_monitor(int k) {
    double hz = gfx_window_refresh_hz();
    if (!G.by_limit || hz <= 0) return k;
    int most = (int)(hz / 60 + 0.05);
    if (most < 1) most = 1;
    if (k <= most) return k;
    if (!G.cap_logged++)
        rt_log("frame generation: the frame limit asks for x%d, the %.0f Hz monitor shows x%d", k, hz, most);
    return most;
}

// The present thread's hold (vk_backend.c, hold_generation): 1 while the game misses its own frame rate, when the
// pictures in between would be paid for with game frames. Any thread reads it.
static int g_hold;

void vk_framegen_hold(int on) { __atomic_store_n(&g_hold, on, __ATOMIC_RELAXED); }

int vk_framegen_pictures(uint64_t period_ns) {
    read_settings();
    if (__atomic_load_n(&g_hold, __ATOMIC_RELAXED)) return G.last_k = 1;
    int k = G.factor;
    if (!k) {
        double hz = gfx_window_refresh_hz();
        k = hz > 0 ? (int)(hz * (double)period_ns / 1e9 + 0.05) : 1;   // 0.05: a period measured a little short
    } else k = cap_to_monitor(k);
    k = k < 1 ? 1 : k > FG_MAX ? FG_MAX : k;
    G.last_k = k;
    return k;
}

int vk_framegen_factor(void) {
    read_settings();
    if (__atomic_load_n(&g_hold, __ATOMIC_RELAXED)) return 1;
    return G.factor ? cap_to_monitor(G.factor) : vk_framegen_pictures(1000000000ull / 60);
}

int vk_framegen_extrapolates(void) {
    read_settings();
    return G.extrap;
}

static int make_pass(VkCtx *c, Pass *p, const uint32_t *spv, size_t bytes, uint32_t n, const VkDescriptorType *types,
                     uint32_t push_bytes) {
    VkDescriptorSetLayoutBinding b[6];
    for (uint32_t i = 0; i < n; i++)
        b[i] = (VkDescriptorSetLayoutBinding){.binding = i, .descriptorType = types[i], .descriptorCount = 1,
                                              .stageFlags = VK_SHADER_STAGE_COMPUTE_BIT};
    VkDescriptorSetLayoutCreateInfo dci = {.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
                                           .flags = VK_DESCRIPTOR_SET_LAYOUT_CREATE_PUSH_DESCRIPTOR_BIT_KHR,
                                           .bindingCount = n, .pBindings = b};
    if (vkCreateDescriptorSetLayout(c->device, &dci, NULL, &p->dsl) != VK_SUCCESS) return 0;
    VkPushConstantRange pr = {VK_SHADER_STAGE_COMPUTE_BIT, 0, push_bytes};
    VkPipelineLayoutCreateInfo li = {.sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO, .setLayoutCount = 1,
                                     .pSetLayouts = &p->dsl, .pushConstantRangeCount = 1, .pPushConstantRanges = &pr};
    if (vkCreatePipelineLayout(c->device, &li, NULL, &p->layout) != VK_SUCCESS) return 0;
    VkShaderModuleCreateInfo mi = {.sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO, .codeSize = bytes, .pCode = spv};
    VkShaderModule m = VK_NULL_HANDLE;
    if (vkCreateShaderModule(c->device, &mi, NULL, &m) != VK_SUCCESS) return 0;
    VkComputePipelineCreateInfo ci = {.sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
                                      .stage = {.sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                                                .stage = VK_SHADER_STAGE_COMPUTE_BIT, .module = m, .pName = "main"},
                                      .layout = p->layout};
    VkResult r = vkCreateComputePipelines(c->device, VK_NULL_HANDLE, 1, &ci, NULL, &p->pipe);
    vkDestroyShaderModule(c->device, m, NULL);
    return r == VK_SUCCESS;
}

static void free_pass(VkCtx *c, Pass *p) {
    if (p->pipe) vkDestroyPipeline(c->device, p->pipe, NULL);
    if (p->layout) vkDestroyPipelineLayout(c->device, p->layout, NULL);
    if (p->dsl) vkDestroyDescriptorSetLayout(c->device, p->dsl, NULL);
    memset(p, 0, sizeof *p);
}

static int init(VkCtx *c) {
    G.tried = 1;
    G.push = (PFN_vkCmdPushDescriptorSetKHR)vkGetDeviceProcAddr(c->device, "vkCmdPushDescriptorSetKHR");
    const VkDescriptorType S = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, W = VK_DESCRIPTOR_TYPE_STORAGE_IMAGE,
                           B = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER;
    const VkDescriptorType t_ingest[6] = {S, W, W, S, W, B}, t_down[2] = {S, W}, t_four[4] = {S, S, S, W},
                           t_synth[6] = {S, S, S, W, S, B};
    VkBufferCreateInfo bi = {.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO, .size = 2 * KEEP * sizeof(uint32_t),
                             .usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT | VK_BUFFER_USAGE_TRANSFER_DST_BIT};
    VmaAllocationCreateInfo bai = {.flags = VMA_ALLOCATION_CREATE_HOST_ACCESS_RANDOM_BIT | VMA_ALLOCATION_CREATE_MAPPED_BIT,
                                   .usage = VMA_MEMORY_USAGE_AUTO};
    VmaAllocationInfo binfo;
    VkSamplerCreateInfo si = {.sType = VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO, .magFilter = VK_FILTER_NEAREST,
                              .minFilter = VK_FILTER_NEAREST, .mipmapMode = VK_SAMPLER_MIPMAP_MODE_NEAREST,
                              .addressModeU = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                              .addressModeV = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                              .addressModeW = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE, .maxLod = 0};
    VkSamplerCreateInfo sl = si;
    sl.magFilter = sl.minFilter = VK_FILTER_LINEAR;
    VkCommandPoolCreateInfo pi = {.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
                                  .flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
                                  .queueFamilyIndex = c->gfx_family};
    int ok = G.push && make_pass(c, &G.ingest, fg_ingest_spv, sizeof fg_ingest_spv, 6, t_ingest, 20) &&
             make_pass(c, &G.down, fg_down_spv, sizeof fg_down_spv, 2, t_down, 16) &&
             make_pass(c, &G.search, fg_flow_spv, sizeof fg_flow_spv, 4, t_four, 32) &&
             make_pass(c, &G.synth, fg_synth_spv, sizeof fg_synth_spv, 6, t_synth, 40) &&
             vkCreateSampler(c->device, &si, NULL, &G.nearest) == VK_SUCCESS &&
             vkCreateSampler(c->device, &sl, NULL, &G.linear) == VK_SUCCESS &&
             vkCreateCommandPool(c->device, &pi, NULL, &G.pool) == VK_SUCCESS &&
             vmaCreateBuffer(c->vma, &bi, &bai, &G.stats, &G.stats_alloc, &binfo) == VK_SUCCESS;
    if (ok) G.stats_map = binfo.pMappedData;
    ok = ok && G.stats_map;
    if (ok) {
        VkCommandBufferAllocateInfo ai = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO, .commandPool = G.pool,
                                          .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY, .commandBufferCount = RING};
        ok = vkAllocateCommandBuffers(c->device, &ai, G.cb) == VK_SUCCESS;
        VkFenceCreateInfo fi = {.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO, .flags = VK_FENCE_CREATE_SIGNALED_BIT};
        for (int i = 0; ok && i < RING; i++) ok = vkCreateFence(c->device, &fi, NULL, &G.fence[i]) == VK_SUCCESS;
    }
    if (!ok) {
        rt_log("vk: frame generation off: its pipelines could not be created");
        return 0;
    }
    rt_log("vk: frame generation ready");
    return G.ok = 1;
}

static void free_img(VkCtx *c, Img *m) {
    if (m->view) vkDestroyImageView(c->device, m->view, NULL);
    if (m->img) vmaDestroyImage(c->vma, m->img, m->alloc);
    memset(m, 0, sizeof *m);
}

static void free_images(VkCtx *c) {
    for (int i = 0; i < KEEP; i++)
        free_img(c, &G.hist[i]), free_img(c, &G.scene[i]), free_img(c, &G.lq[i]), free_img(c, &G.ls[i]),
            free_img(c, &G.flow[i]), G.prev[i] = -1;
    free_img(c, &G.flow0), free_img(c, &G.out);
    G.w = G.h = 0;
    G.newest = -1;
}

static int make_img_usage(VkCtx *c, Img *m, VkFormat fmt, uint32_t w, uint32_t h, VkImageUsageFlags usage) {
    VkImageCreateInfo ii = {.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO, .imageType = VK_IMAGE_TYPE_2D, .format = fmt,
                            .extent = {w, h, 1}, .mipLevels = 1, .arrayLayers = 1, .samples = VK_SAMPLE_COUNT_1_BIT,
                            .tiling = VK_IMAGE_TILING_OPTIMAL, .usage = usage};
    VmaAllocationCreateInfo ai = {.usage = VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE};
    if (vmaCreateImage(c->vma, &ii, &ai, &m->img, &m->alloc, NULL) != VK_SUCCESS) {
        m->img = VK_NULL_HANDLE;
        return 0;
    }
    VkImageViewCreateInfo vi = {.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO, .image = m->img,
                                .viewType = VK_IMAGE_VIEW_TYPE_2D, .format = fmt,
                                .subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1}};
    return vkCreateImageView(c->device, &vi, NULL, &m->view) == VK_SUCCESS;
}

static int make_img(VkCtx *c, Img *m, VkFormat fmt, uint32_t w, uint32_t h) {
    return make_img_usage(c, m, fmt, w, h,
                          VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT);
}

static int ensure_images(VkCtx *c, uint32_t w, uint32_t h) {
    if (G.hist[0].img && G.w == w && G.h == h) return 1;
    // another crop size (a resolution change): earlier submissions may still read the old images
    vk_queue_lock();
    vkDeviceWaitIdle(c->device);
    vk_queue_unlock();
    free_images(c);
    uint32_t qw = (w + 3) / 4, qh = (h + 3) / 4, gw = (qw + 3) / 4, gh = (qh + 3) / 4;
    const VkFormat RGBA = VK_FORMAT_R16G16B16A16_SFLOAT, R = VK_FORMAT_R16_SFLOAT, RG = VK_FORMAT_R16G16_SFLOAT;
    int ok = 1;
    for (int i = 0; ok && i < KEEP; i++)
        ok = make_img(c, &G.hist[i], RGBA, w, h) && make_img(c, &G.scene[i], RGBA, w, h) &&
             make_img(c, &G.lq[i], R, qw, qh) && make_img(c, &G.ls[i], R, gw, gh) && make_img(c, &G.flow[i], RG, gw, gh);
    ok = ok && make_img(c, &G.flow0, RG, gw, gh) && make_img(c, &G.out, RGBA, w, h);
    if (!ok) {
        rt_log("vk: frame generation off: no memory for its %ux%u images", w, h);
        free_images(c);
        return 0;
    }
    G.w = w, G.h = h, G.qw = qw, G.qh = qh, G.gw = gw, G.gh = gh;
    rt_log("vk: frame generation at %ux%u, motion field %ux%u", w, h, gw, gh);
    return 1;
}

static void barrier(VkCommandBuffer cb, VkImage img, VkImageLayout from, VkPipelineStageFlags2 ss, VkAccessFlags2 sa,
                    VkPipelineStageFlags2 ds, VkAccessFlags2 da) {
    VkImageMemoryBarrier2 b = {.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2, .srcStageMask = ss, .srcAccessMask = sa,
                               .dstStageMask = ds, .dstAccessMask = da, .oldLayout = from,
                               .newLayout = VK_IMAGE_LAYOUT_GENERAL, .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
                               .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED, .image = img,
                               .subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1}};
    VkDependencyInfo d = {.sType = VK_STRUCTURE_TYPE_DEPENDENCY_INFO, .imageMemoryBarrierCount = 1,
                          .pImageMemoryBarriers = &b};
    vkCmdPipelineBarrier2(cb, &d);
}

static const VkPipelineStageFlags2 CS = VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT, ALL = VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT;
static const VkAccessFlags2 WR = VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT, RD = VK_ACCESS_2_SHADER_SAMPLED_READ_BIT;

// An image a pass is about to overwrite whole: after every earlier use on the queue, old content dropped
static void will_write(VkCommandBuffer cb, const Img *m) { barrier(cb, m->img, VK_IMAGE_LAYOUT_UNDEFINED, ALL, 0, CS, WR); }
// An image a pass wrote, for the next compute pass to sample
static void written(VkCommandBuffer cb, const Img *m) { barrier(cb, m->img, VK_IMAGE_LAYOUT_GENERAL, CS, WR, CS, RD); }
// An image a pass wrote, for anything later on the queue (the synth, the present's blit or colour pass)
static void published(VkCommandBuffer cb, const Img *m) {
    barrier(cb, m->img, VK_IMAGE_LAYOUT_GENERAL, CS, WR, ALL, VK_ACCESS_2_MEMORY_READ_BIT);
}

// Binds pass p with n image descriptors: views[i] sampled (sampler != 0) or storage (sampler == 0), and buf (if any)
// as the storage buffer of binding n, then dispatches
static void run(VkCommandBuffer cb, const Pass *p, uint32_t n, const VkImageView *views, const VkSampler *samplers,
                VkBuffer buf, const void *pc, uint32_t pc_bytes, uint32_t gx, uint32_t gy) {
    VkDescriptorImageInfo ii[6];
    VkWriteDescriptorSet w[7];
    VkDescriptorBufferInfo bi = {buf, 0, VK_WHOLE_SIZE};
    for (uint32_t i = 0; i < n; i++) {
        ii[i] = (VkDescriptorImageInfo){samplers[i], views[i], VK_IMAGE_LAYOUT_GENERAL};
        w[i] = (VkWriteDescriptorSet){.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = i, .descriptorCount = 1,
                                      .descriptorType = samplers[i] ? VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER
                                                                    : VK_DESCRIPTOR_TYPE_STORAGE_IMAGE,
                                      .pImageInfo = &ii[i]};
    }
    if (buf)
        w[n] = (VkWriteDescriptorSet){.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = n, .descriptorCount = 1,
                                      .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_BUFFER, .pBufferInfo = &bi};
    vkCmdBindPipeline(cb, VK_PIPELINE_BIND_POINT_COMPUTE, p->pipe);
    rec_push(G.push, cb, VK_PIPELINE_BIND_POINT_COMPUTE, p->layout, 0, n + (buf ? 1 : 0), w);
    vkCmdPushConstants(cb, p->layout, VK_SHADER_STAGE_COMPUTE_BIT, 0, pc_bytes, pc);
    vkCmdDispatch(cb, (gx + 7) / 8, (gy + 7) / 8, 1);
}

// The next ring slot's command buffer, begun, once its previous submission is done; NULL on a device error
static VkCommandBuffer begin(VkCtx *c, unsigned *slot) {
    *slot = G.next++ % RING;
    if (vkWaitForFences(c->device, 1, &G.fence[*slot], VK_TRUE, 5000000000ull) != VK_SUCCESS) {
        rt_log("vk: frame generation: fence wait timed out");
        return VK_NULL_HANDLE;
    }
    vkResetFences(c->device, 1, &G.fence[*slot]);
    VkCommandBuffer cb = G.cb[*slot];
    vkResetCommandBuffer(cb, 0);
    VkCommandBufferBeginInfo bi = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
                                   .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT};
    vkBeginCommandBuffer(cb, &bi);
    return cb;
}

static int submit(VkCtx *c, unsigned slot) {
    vkEndCommandBuffer(G.cb[slot]);
    VkCommandBufferSubmitInfo cbi = {.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_SUBMIT_INFO, .commandBuffer = G.cb[slot]};
    VkSubmitInfo2 si = {.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO_2, .commandBufferInfoCount = 1, .pCommandBufferInfos = &cbi};
    vk_queue_lock();
    VkResult r = vkQueueSubmit2(c->gfx_queue, 1, &si, G.fence[slot]);
    vk_queue_unlock();
    if (r != VK_SUCCESS) {
        vk_device_fault_report(r);
        rt_log("vk: frame generation submit failed (%d)", (int)r);
        // the fence stays unsignalled: signal it with an empty submission so the slot can be used again
        vk_queue_lock();
        vkQueueSubmit2(c->gfx_queue, 0, NULL, G.fence[slot]);
        vk_queue_unlock();
        return 0;
    }
    return 1;
}

// Returns the frame's id for vk_framegen_present (its index + 1), 0 when not taken
int vk_framegen_ingest(VkImage src, VkFormat fmt, int w, int h, const int crop[4]) {
    if (vk_framegen_factor() < 2) return 0;
    VkCtx *c = vk_ctx();
    if (!c) return 0;
    if (!G.tried) init(c);
    if (!G.ok) return 0;
    int cx = 0, cy = 0, cw = w, ch = h;
    if (crop && crop[2] > 0 && crop[3] > 0) cx = crop[0], cy = crop[1], cw = crop[2], ch = crop[3];
    if (cx < 0) cx = 0;
    if (cy < 0) cy = 0;
    if (cx + cw > w) cw = w - cx;
    if (cy + ch > h) ch = h - cy;
    if (cw < 64 || ch < 64 || !ensure_images(c, (uint32_t)cw, (uint32_t)ch)) return 0;
    unsigned slot;
    VkCommandBuffer cb = begin(c, &slot);
    if (!cb) return 0;
    if (G.src_view[slot]) vkDestroyImageView(c->device, G.src_view[slot], NULL);
    VkImageViewCreateInfo vi = {.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO, .image = src,
                                .viewType = VK_IMAGE_VIEW_TYPE_2D, .format = fmt,
                                .subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1}};
    if (vkCreateImageView(c->device, &vi, NULL, &G.src_view[slot]) != VK_SUCCESS) {
        G.src_view[slot] = VK_NULL_HANDLE;
        submit(c, slot);   // empty: keeps the slot's fence going
        return 0;
    }
    int prev = G.newest, cur = (prev + 1) % KEEP;   // prev -1: the first frame since the images were made
    // the game's writes to src are visible to the ingest; the new frame's images are free
    barrier(cb, src, VK_IMAGE_LAYOUT_GENERAL, ALL, VK_ACCESS_2_MEMORY_WRITE_BIT, CS, RD);
    will_write(cb, &G.hist[cur]);
    will_write(cb, &G.lq[cur]);
    struct { int32_t offset[2], size[2], slot, encode; } pi = {{cx, cy}, {cw, ch}, cur, 0};
    // the scene before the HUD, when this frame's replay copied it at the picture's size
    HudSlot *hs = hud_slot(src);
    if (hs && (hs->w != (uint32_t)w || hs->h != (uint32_t)h)) hs = NULL;
    // the count of the frame that used this counter last (KEEP frames ago, its ingest long done), for the log; then
    // zeroed for this one
    if (G.stats_pending[cur]) {
        vmaInvalidateAllocation(c->vma, G.stats_alloc, 0, VK_WHOLE_SIZE);
        uint32_t lin = G.stats_map[2 * cur], enc = G.stats_map[2 * cur + 1];
        float px = (float)G.w * (float)G.h, best = (float)(enc <= lin ? enc : lin) / px;
        // the encoding follows only a count that matches (a menu or a scene drawn another way matches neither)
        if (best <= HUD_LIMIT && G.encode != (enc <= lin)) {
            rt_log("vk: frame generation: the picture matches its scene %s (%u and %u differing pixels)",
                   enc <= lin ? "sRGB-encoded" : "as it is", lin, enc);
            G.encode = enc <= lin;
        }
        float share = (float)(G.encode ? enc : lin) / px;
        G.log_hud += share, G.log_read++;
        G.log_mismatch += share > HUD_LIMIT;
        G.stats_pending[cur] = 0;
    }
    G.log_frames++, G.log_scenes += hs != NULL;
    vkCmdFillBuffer(cb, G.stats, (VkDeviceSize)(2 * cur) * sizeof(uint32_t), 2 * sizeof(uint32_t), 0);
    pi.encode = hs ? G.encode : 0;
    VkMemoryBarrier2 mb = {.sType = VK_STRUCTURE_TYPE_MEMORY_BARRIER_2, .srcStageMask = ALL,
                           .srcAccessMask = VK_ACCESS_2_MEMORY_WRITE_BIT, .dstStageMask = CS,
                           .dstAccessMask = VK_ACCESS_2_SHADER_STORAGE_READ_BIT | VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT};
    VkDependencyInfo md = {.sType = VK_STRUCTURE_TYPE_DEPENDENCY_INFO, .memoryBarrierCount = 1, .pMemoryBarriers = &mb};
    vkCmdPipelineBarrier2(cb, &md);
    if (hs) barrier(cb, hs->img.img, VK_IMAGE_LAYOUT_GENERAL, ALL, VK_ACCESS_2_MEMORY_WRITE_BIT, CS, RD);
    will_write(cb, &G.scene[cur]);
    VkImageView v_in[5] = {G.src_view[slot], G.hist[cur].view, G.lq[cur].view, hs ? hs->img.view : G.src_view[slot],
                           G.scene[cur].view};
    VkSampler s_in[5] = {G.nearest, VK_NULL_HANDLE, VK_NULL_HANDLE, G.nearest, VK_NULL_HANDLE};
    run(cb, &G.ingest, 5, v_in, s_in, G.stats, &pi, sizeof pi, G.qw, G.qh);
    // the count for the synths and the host
    mb = (VkMemoryBarrier2){.sType = VK_STRUCTURE_TYPE_MEMORY_BARRIER_2, .srcStageMask = CS,
                            .srcAccessMask = VK_ACCESS_2_SHADER_STORAGE_WRITE_BIT,
                            .dstStageMask = ALL | VK_PIPELINE_STAGE_2_HOST_BIT,
                            .dstAccessMask = VK_ACCESS_2_MEMORY_READ_BIT | VK_ACCESS_2_HOST_READ_BIT};
    vkCmdPipelineBarrier2(cb, &md);
    barrier(cb, src, VK_IMAGE_LAYOUT_GENERAL, CS, 0, ALL, 0);   // later writes of the game image wait for this read
    published(cb, &G.hist[cur]);
    published(cb, &G.scene[cur]);
    written(cb, &G.lq[cur]);
    will_write(cb, &G.ls[cur]);
    struct { int32_t size[2], src_size[2]; } pd = {{(int32_t)G.gw, (int32_t)G.gh}, {(int32_t)G.qw, (int32_t)G.qh}};
    VkImageView v_dn[2] = {G.lq[cur].view, G.ls[cur].view};
    VkSampler s_dn[2] = {G.nearest, VK_NULL_HANDLE};
    run(cb, &G.down, 2, v_dn, s_dn, VK_NULL_HANDLE, &pd, sizeof pd, G.gw, G.gh);
    written(cb, &G.ls[cur]);
    if (prev >= 0) {   // the earlier frame is there: the motion between the two
        struct { int32_t grid[2], lsize[2], scale, radius, half_window, refine; } pf = {
            {(int32_t)G.gw, (int32_t)G.gh}, {(int32_t)G.gw, (int32_t)G.gh}, 16, 8, 2, 0};
        will_write(cb, &G.flow0);
        // the coarse pass reads no prior field: the newer luma stands in for the binding
        VkImageView v_c[4] = {G.ls[prev].view, G.ls[cur].view, G.ls[cur].view, G.flow0.view};
        VkSampler s_c[4] = {G.nearest, G.nearest, G.nearest, VK_NULL_HANDLE};
        run(cb, &G.search, 4, v_c, s_c, VK_NULL_HANDLE, &pf, sizeof pf, G.gw, G.gh);
        written(cb, &G.flow0);
        will_write(cb, &G.flow[cur]);
        pf.lsize[0] = (int32_t)G.qw, pf.lsize[1] = (int32_t)G.qh;
        pf.scale = 4, pf.radius = 2, pf.half_window = 4, pf.refine = 1;
        VkImageView v_f[4] = {G.lq[prev].view, G.lq[cur].view, G.flow0.view, G.flow[cur].view};
        run(cb, &G.search, 4, v_f, s_c, VK_NULL_HANDLE, &pf, sizeof pf, G.gw, G.gh);
        published(cb, &G.flow[cur]);
    }
    if (!submit(c, slot)) return 0;
    G.newest = cur;
    G.prev[cur] = prev;
    G.has_scene[cur] = hs != NULL;
    G.stats_pending[cur] = hs != NULL;
    G.enc[cur] = pi.encode;
    return cur + 1;
}

// The rate the performance overlay shows (vk_framegen_shown_rate): written once a second by the present thread
static volatile int32_t rate_milli = -1, rate_factor;
static volatile int64_t rate_at;

float vk_framegen_shown_rate(int *factor) {
    int32_t r = __atomic_load_n(&rate_milli, __ATOMIC_ACQUIRE);
    if (factor) *factor = __atomic_load_n(&rate_factor, __ATOMIC_RELAXED);
    if (r < 0 || kern_now_ns() - (uint64_t)__atomic_load_n(&rate_at, __ATOMIC_RELAXED) > 2000000000ull) return -1;
    return r / 1000.0f;
}

// One log line every 5 s: pictures shown per second, how many of them generated, and game frames taken
static void count(int generated) {
    static uint64_t since, shown, made, since1, shown1;
    uint64_t now = kern_now_ns();
    if (!since1) since1 = now;
    shown1++;
    if (now - since1 >= 1000000000ull) {
        __atomic_store_n(&rate_factor, G.last_k, __ATOMIC_RELAXED);
        __atomic_store_n(&rate_at, (int64_t)now, __ATOMIC_RELAXED);
        __atomic_store_n(&rate_milli, (int32_t)(shown1 * 1e12 / (double)(now - since1)), __ATOMIC_RELEASE);
        since1 = now, shown1 = 0;
    }
    if (!since) since = now;
    shown++, made += generated;
    if (now - since >= 5000000000ull) {
        double s = (now - since) / 1e9;
        rt_log("frame generation: %.1f pictures per second, %.1f of them generated (x%d, %s); scene before the HUD in "
               "%llu of %llu game frames, the HUD %.1f%% of the picture, %llu not matching",
               shown / s, made / s, G.last_k, G.extrap ? "extrap" : "interp", (unsigned long long)G.log_scenes,
               (unsigned long long)G.log_frames,
               G.log_read ? 100.0 * G.log_hud / (double)G.log_read : 0.0, (unsigned long long)G.log_mismatch);
        G.log_frames = G.log_scenes = G.log_read = G.log_mismatch = 0, G.log_hud = 0;
        since = now, shown = made = 0;
    }
}

void vk_framegen_present(int id, float tau) {
    VkCtx *c = vk_ctx();
    int f = id - 1;
    if (!G.ok || !G.w || !c || f < 0 || f >= KEEP) return;
    count(tau != 1.0f && G.prev[f] >= 0);
    // the frame itself, or nothing to generate it from: present its picture as it is
    if (tau == 1.0f || G.prev[f] < 0) {
        vk_present_generated(G.hist[f].img, (int)G.w, (int)G.h);
        return;
    }
    unsigned slot;
    VkCommandBuffer cb = begin(c, &slot);
    if (!cb) return;
    will_write(cb, &G.out);
    int p = G.prev[f];
    struct { int32_t size[2], grid[2]; float tau, occlusion, hud; uint32_t limit; int32_t slot_a, slot_b; } ps = {
        {(int32_t)G.w, (int32_t)G.h}, {(int32_t)G.gw, (int32_t)G.gh}, tau, OCCLUSION,
        G.has_scene[f] && G.has_scene[p] ? 1.0f : 0.0f, (uint32_t)(HUD_LIMIT * (float)G.w * (float)G.h),
        2 * p + G.enc[p], 2 * f + G.enc[f]};
    VkImageView v[5] = {G.scene[p].view, G.scene[f].view, G.flow[f].view, G.out.view, G.hist[f].view};
    VkSampler s[5] = {G.linear, G.linear, G.linear, VK_NULL_HANDLE, G.linear};
    run(cb, &G.synth, 5, v, s, G.stats, &ps, sizeof ps, G.w, G.h);
    published(cb, &G.out);
    if (submit(c, slot)) vk_present_generated(G.out.img, (int)G.w, (int)G.h);
}

void vk_framegen_release(void) {
    VkCtx *c = vk_ctx();
    if (!c || !G.tried) return;
    for (int i = 0; i < RING; i++) {
        if (G.src_view[i]) vkDestroyImageView(c->device, G.src_view[i], NULL);
        if (G.fence[i]) vkDestroyFence(c->device, G.fence[i], NULL);
        G.src_view[i] = VK_NULL_HANDLE, G.fence[i] = VK_NULL_HANDLE;
    }
    free_images(c);
    for (int i = 0; i < HUD_SLOTS; i++) free_img(c, &H.slot[i].img), H.slot[i].dest = 0, H.slot[i].w = 0;
    if (G.stats) vmaDestroyBuffer(c->vma, G.stats, G.stats_alloc);
    G.stats = VK_NULL_HANDLE, G.stats_map = NULL;
    for (int i = 0; i < H.nold; i++) free_img(c, &H.old[i]);
    H.nold = 0;
    if (G.pool) vkDestroyCommandPool(c->device, G.pool, NULL);   // frees the command buffers
    G.pool = VK_NULL_HANDLE;
    free_pass(c, &G.ingest), free_pass(c, &G.down), free_pass(c, &G.search), free_pass(c, &G.synth);
    if (G.nearest) vkDestroySampler(c->device, G.nearest, NULL);
    if (G.linear) vkDestroySampler(c->device, G.linear, NULL);
    G.nearest = G.linear = VK_NULL_HANDLE;
    G.tried = G.ok = 0;
    G.next = 0;
}
