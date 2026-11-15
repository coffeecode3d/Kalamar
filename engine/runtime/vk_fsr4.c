// AMD FidelityFX upscaler provider (FSR 4.x / FSR 3.1.x), loaded at run time. The provider exports the FidelityFX
// ffx API (ffxCreateContext, ffxDispatch, ...); the engine passes its Vulkan device and the upscaler runs on the
// game's 1080p colour, depth and motion vectors, writing the window-sized output. The FSR 1 present path (vk_fsr.c)
// stays the fallback.
//
// Where the provider comes from:
//   Windows: amd_fidelityfx_loader_vk.dll + amd_fidelityfx_upscaler_vk.dll from AMD's FSR SDK (the loader picks the
//            effect DLL next to it); both are redistributable under the SDK's MIT licence.
//   Linux:   AMD publishes no official Vulkan FSR 4 runtime. If the user supplies amd_fidelityfx_upscaler_vk.so
//            next to the executable (a community build such as fsr4-vk), it is loaded here. KLM_FSR4=<path> overrides.
// Nothing is loaded unless the setting is on: KLM_UPSCALER=fsr4 (launcher: Upscaler).
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

#include "cfg.h"
#include "host.h"
#include "platform/platform.h"
#include "vk.h"

#if !defined(_WIN32)
#define __declspec(x) __attribute__((visibility("default")))
#endif
#include "third_party/ffx/upscalers/include/ffx_upscale.h"

// The Vulkan backend create descriptor. FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK_DEPRECATED, which the 4.1.x Vulkan
// provider used, and which community Linux builds implement.
#define FFX_BACKEND_VK_DESC_DEPRECATED 0x0000003u
#define FFX_BACKEND_VK_DESC 0x01000003u   // FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK (SDK 2.4)
typedef struct {
    ffxCreateContextDescHeader header;
    VkDevice device;
    VkPhysicalDevice physical;
    PFN_vkGetDeviceProcAddr getDeviceProcAddr;
} FfxBackendVkDesc;

typedef ffxReturnCode_t (*FfxCreateContextFn)(ffxContext *, ffxCreateContextDescHeader *,
                                              const ffxAllocationCallbacks *);
typedef ffxReturnCode_t (*FfxDestroyContextFn)(ffxContext *, const ffxAllocationCallbacks *);
typedef ffxReturnCode_t (*FfxQueryFn)(ffxContext *, ffxQueryDescHeader *);
typedef ffxReturnCode_t (*FfxDispatchFn)(ffxContext *, ffxDispatchDescHeader *);
typedef ffxReturnCode_t (*FfxConfigureFn)(ffxContext *, ffxConfigureDescHeader *);

static void fsr_message(uint32_t type, const wchar_t *message) {
    char buf[512];
    if (message && wcstombs(buf, message, sizeof buf) != (size_t)-1) rt_log("fsr4: %s: %s", type ? "warning" : "error", buf);
}

static struct {
    void *lib;
    ffxContext ctx;
    FfxCreateContextFn create;
    FfxDestroyContextFn destroy;
    FfxQueryFn query;
    FfxDispatchFn dispatch;
    FfxConfigureFn configure;
    int tried, ok;
    char version[40];   // the provider's own name for what it runs ("FSR 4.1.1"), empty: it does not say
} U;

static void provider_path(char *out, size_t cap) {
    const char *env = getenv("KLM_FSR4");
    if (env && *env) {
        snprintf(out, cap, "%s", env);
        return;
    }
    char exe[1024];
    if (!plat_exe_path(exe, sizeof exe)) {
        out[0] = 0;
        return;
    }
    char *sep = plat_last_sep(exe);
    if (!sep) {
        out[0] = 0;
        return;
    }
#if defined(_WIN32)
    snprintf(out, cap, "%.*s\\amd_fidelityfx_loader_vk.dll", (int)(sep - exe), exe);
    if (plat_file_exists(out)) return;
    snprintf(out, cap, "%.*s\\amd_fidelityfx_upscaler_vk.dll", (int)(sep - exe), exe);
#else
    snprintf(out, cap, "%.*s/amd_fidelityfx_upscaler_vk.so", (int)(sep - exe), exe);
#endif
}

static void *sym_required(void *lib, const char *name, const char *what) {
    void *p = plat_lib_sym(lib, name);
    if (!p) rt_log("fsr4: %s: %s missing from the provider", what, name);
    return p;
}

// The menu's FSR 4.1 row asks for the newest version the provider has, the FSR 3.1 row for the 3.x one. AMD's loader
// and its effect runners answer FFX_API_QUERY_DESC_TYPE_GET_VERSIONS with the versions they carry ("FSR 4.1.1",
// "FSR 3.1.5", ...), and a ffxOverrideVersion in the create chain picks one; a provider without the query (the Linux
// community builds) leaves the choice to itself, which is what they did before this row existed.
static int version_major(void) {
    const char *u = cfg_get("TOTK_UPSCALER");
    return u && !strcmp(u, "fsr3") ? 3 : 4;
}

// The id of the newest version whose name starts with the wanted major ("FSR 4..." / "FSR 3..."), 0: none or no query.
static uint64_t pick_version(void) {
    if (!U.query) return 0;
    uint64_t n = 0;
    struct ffxQueryDescGetVersions q = {0};
    q.header.type = FFX_API_QUERY_DESC_TYPE_GET_VERSIONS;
    q.createDescType = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE;
    if (U.query(NULL, &q.header) != FFX_API_RETURN_OK || !n) return 0;   // the count lands in outputCount (&n)
    if (n > 16) n = 16;
    uint64_t ids[16];
    const char *names[16];
    memset(names, 0, sizeof names);
    uint64_t cap = n;
    q.outputCount = &cap, q.versionIds = ids, q.versionNames = names;
    if (U.query(NULL, &q.header) != FFX_API_RETURN_OK) return 0;
    int want = version_major();
    uint64_t pick = 0;
    char have[200] = "";
    for (uint64_t i = 0; i < cap; i++) {
        const char *nm = names[i] ? names[i] : "?";
        if (strlen(have) + strlen(nm) + 3 < sizeof have) strcat(have, have[0] ? ", " : ""), strcat(have, nm);
        const char *d = nm;
        while (*d && (*d < '0' || *d > '9')) d++;
        if (*d - '0' == want) pick = ids[i];   // newest first from AMD's loader; the last match is the oldest of that major
    }
    if (have[0]) rt_log("fsr4: the provider offers %s (taking %s %d.x)", have, pick ? "FSR" : "no", want);
    if (pick) {   // remember the name for the overlay: the one it picked
        for (uint64_t i = 0; i < cap; i++)
            if (ids[i] == pick && names[i] && strlen(names[i]) < sizeof U.version)
                snprintf(U.version, sizeof U.version, "%s", names[i]);
    }
    return pick;
}

// Loads the provider and creates an upscale context for a 1920x1080 render size and the given output size. Idempotent;
// 1 when the context is ready.
int vk_fsr4_init(void) {
    if (U.tried) return U.ok;
    U.tried = 1;
    const char *want = cfg_get("TOTK_UPSCALER");
    if (!want || (strcmp(want, "fsr4") && strcmp(want, "fsr3"))) return 0;
    char path[1200];
    provider_path(path, sizeof path);
    if (!path[0] || !plat_file_exists(path)) {
#if defined(_WIN32)
        rt_log("fsr4: no amd_fidelityfx_loader_vk.dll next to the executable (looked for %s): FSR 1 for now; a runtime "
               "added later needs a restart", path[0] ? path : "?");
#else
        rt_log("fsr4: no amd_fidelityfx_upscaler_vk.so at %s: FSR 4.1/3.1 falls back to FSR 1 (set KLM_FSR4=<path> to "
               "point at one); a runtime added later needs a restart", path[0] ? path : "?");
#endif
        return 0;
    }
    U.lib = plat_lib_open(path);
    if (!U.lib) {
        char why[256];
        plat_lib_error(why, sizeof why);
        rt_log("fsr4: cannot load %s: %s", path, why);
        return 0;
    }
    U.create = (FfxCreateContextFn)sym_required(U.lib, "ffxCreateContext", "create");
    U.destroy = (FfxDestroyContextFn)sym_required(U.lib, "ffxDestroyContext", "destroy");
    U.query = (FfxQueryFn)sym_required(U.lib, "ffxQuery", "query");
    U.dispatch = (FfxDispatchFn)sym_required(U.lib, "ffxDispatch", "dispatch");
    U.configure = (FfxConfigureFn)sym_required(U.lib, "ffxConfigure", "configure");
    if (!U.create || !U.destroy || !U.dispatch) return 0;
    FfxBackendVkDesc backend = {.device = vk_ctx()->device,
                                .physical = vk_ctx()->phys,
                                .getDeviceProcAddr = vkGetDeviceProcAddr};
    struct ffxCreateContextDescUpscale up = {.header = {.type = FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE},
                                      .flags = 0,
                                      .maxRenderSize = {1920, 1080},
                                      .maxUpscaleSize = {3840, 2160},
                                      .fpMessage = fsr_message};
    struct ffxOverrideVersion over = {.header = {.type = FFX_API_DESC_TYPE_OVERRIDE_VERSION}, .versionId = pick_version()};
    if (over.versionId) over.header.pNext = &backend.header, up.header.pNext = &over.header;
    else up.header.pNext = &backend.header;   // the upscale desc is the head; the Vulkan backend follows it
    backend.header.type = FFX_BACKEND_VK_DESC;   // AMD's SDK 2.4 loader
    ffxReturnCode_t r = U.create(&U.ctx, &up.header, NULL);
    if (r != FFX_API_RETURN_OK) {   // the older/deprecated descriptor that the Linux community builds take
        U.ctx = NULL;
        backend.header.type = FFX_BACKEND_VK_DESC_DEPRECATED;
        r = U.create(&U.ctx, &up.header, NULL);
    }
    if (r != FFX_API_RETURN_OK) {
        rt_log("fsr4: ffxCreateContext failed (%u); upscaler stays FSR 1", (unsigned)r);
        U.ctx = NULL;
        return 0;
    }
    U.ok = 1;
    rt_log("fsr4: provider ready: %s%s%s (render 1920x1080, up to 3840x2160)", path, U.version[0] ? ", " : "",
           U.version);
    return 1;
}

// What the provider runs, for the overlay ("FSR 4.1.1"); "" when it does not say.
const char *vk_fsr4_version(void) { return U.version; }

int vk_fsr4_ready(void) {
    const char *want = cfg_get("TOTK_UPSCALER");
    if (!want || (strcmp(want, "fsr4") && strcmp(want, "fsr3"))) return 0;
    if (!U.tried) vk_fsr4_init();   // the menu can pick it after the device exists
    return U.ok;
}

// ---------------------------------------------------------------- motion vectors and dispatch
#include "dlaa_mv_spv.h"
#include "fsr4_color_spv.h"

static struct {
    int ok;
    VkImage mv;
    VmaAllocation mv_mem;
    VkImageView mv_view;
    uint32_t mw, mh;
    VkBuffer ubo;
    VmaAllocation ubo_mem;
    uint8_t *ubo_map;
    VkImage dummy;
    VmaAllocation dummy_mem;
    VkImageView dummy_view;
    VkDescriptorSetLayout dsl;
    VkPipelineLayout pl;
    VkPipeline pipe;
    VkSampler samp;
    PFN_vkCmdPushDescriptorSetKHR push;
    VkImage depth;
    VkFormat depth_fmt;
    VkImageView depth_view;
    double prev_vp[16];          // the view-projection of the frame the provider last took
    double vp[16];               // this frame's, waiting for the dispatch
    uint64_t last_frame;         // gfx_counters.frames of the last frame the provider took
    int prev_ok;
    int resized;                 // a picture or the vectors changed size: the history is from another resolution
    VkImage out;
    VmaAllocation out_mem;
    VkImageView out_view;
    uint32_t ow, oh;
    int logged;
    VkImage col;
    VmaAllocation col_mem;
    VkImageView col_view;
    uint32_t cw, ch;
    VkImage exp;
    VmaAllocation exp_mem;
    int exp_ready;
    VkImage src;
    VkFormat src_fmt;
    VkImageView src_view;
    VkDescriptorSetLayout cdsl;
    VkPipelineLayout cpl;
    VkPipeline cpipe;
} M;

static int inv4(const double m[16], double o[16]) {
    double a[4][8];
    for (int r = 0; r < 4; r++)
        for (int k = 0; k < 8; k++) a[r][k] = k < 4 ? m[r * 4 + k] : (k - 4 == r);
    for (int col = 0; col < 4; col++) {
        int piv = col;
        for (int r = col + 1; r < 4; r++)
            if (fabs(a[r][col]) > fabs(a[piv][col])) piv = r;
        if (fabs(a[piv][col]) < 1e-300) return 0;
        for (int k = 0; k < 8; k++) { double t = a[col][k]; a[col][k] = a[piv][k]; a[piv][k] = t; }
        double d = a[col][col];
        for (int k = 0; k < 8; k++) a[col][k] /= d;
        for (int r = 0; r < 4; r++)
            if (r != col) {
                double f = a[r][col];
                for (int k = 0; k < 8; k++) a[r][k] -= f * a[col][k];
            }
    }
    for (int r = 0; r < 4; r++)
        for (int k = 0; k < 4; k++) o[r * 4 + k] = a[r][k + 4];
    return 1;
}

static int make_image(uint32_t w, uint32_t h, VkFormat f, VkImageUsageFlags use, VkImage *img, VmaAllocation *mem,
                      VkImageView *view) {
    const VkCtx *c = vk_ctx();
    VkImageCreateInfo ii = {.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
                            .imageType = VK_IMAGE_TYPE_2D,
                            .format = f,
                            .extent = {w, h, 1},
                            .mipLevels = 1,
                            .arrayLayers = 1,
                            .samples = VK_SAMPLE_COUNT_1_BIT,
                            .tiling = VK_IMAGE_TILING_OPTIMAL,
                            .usage = use,
                            .initialLayout = VK_IMAGE_LAYOUT_GENERAL};
    VmaAllocationCreateInfo ai = {.usage = VMA_MEMORY_USAGE_AUTO_PREFER_DEVICE};
    if (vmaCreateImage(c->vma, &ii, &ai, img, mem, NULL) != VK_SUCCESS) return 0;
    VkImageViewCreateInfo vi = {.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                                .image = *img,
                                .viewType = VK_IMAGE_VIEW_TYPE_2D,
                                .format = f,
                                .subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1}};
    return vkCreateImageView(c->device, &vi, NULL, view) == VK_SUCCESS;
}

static void full_barrier(VkCommandBuffer cb) {
    VkMemoryBarrier2 b = {.sType = VK_STRUCTURE_TYPE_MEMORY_BARRIER_2,
                          .srcStageMask = VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT,
                          .srcAccessMask = VK_ACCESS_2_MEMORY_WRITE_BIT,
                          .dstStageMask = VK_PIPELINE_STAGE_2_ALL_COMMANDS_BIT,
                          .dstAccessMask = VK_ACCESS_2_MEMORY_READ_BIT | VK_ACCESS_2_MEMORY_WRITE_BIT};
    VkDependencyInfo d = {.sType = VK_STRUCTURE_TYPE_DEPENDENCY_INFO, .memoryBarrierCount = 1, .pMemoryBarriers = &b};
    vkCmdPipelineBarrier2(cb, &d);
}

static int ensure_depth_view(VkImage img, VkFormat fmt) {
    if (M.depth_view && M.depth == img && M.depth_fmt == fmt) return 1;
    if (M.depth_view) vkDestroyImageView(vk_ctx()->device, M.depth_view, NULL), M.depth_view = VK_NULL_HANDLE;
    VkImageViewCreateInfo vi = {.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                                .image = img,
                                .viewType = VK_IMAGE_VIEW_TYPE_2D,
                                .format = fmt,
                                .subresourceRange = {VK_IMAGE_ASPECT_DEPTH_BIT, 0, 1, 0, 1}};
    if (vkCreateImageView(vk_ctx()->device, &vi, NULL, &M.depth_view) != VK_SUCCESS) return 0;
    M.depth = img, M.depth_fmt = fmt;
    return 1;
}

static int color_init(void) {
    const VkCtx *c = vk_ctx();
    if (M.cpipe) return 1;
    VkDescriptorSetLayoutBinding b[2] = {
        {0, VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL},
        {1, VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL}};
    VkDescriptorSetLayoutCreateInfo dci = {.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
                                           .flags = VK_DESCRIPTOR_SET_LAYOUT_CREATE_PUSH_DESCRIPTOR_BIT_KHR,
                                           .bindingCount = 2, .pBindings = b};
    VkPushConstantRange pr = {VK_SHADER_STAGE_COMPUTE_BIT, 0, 16};
    VkShaderModuleCreateInfo mi = {.sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
                                   .codeSize = sizeof fsr4_color_spv,
                                   .pCode = fsr4_color_spv};
    VkShaderModule m = VK_NULL_HANDLE;
    if (vkCreateDescriptorSetLayout(c->device, &dci, NULL, &M.cdsl) != VK_SUCCESS) return 0;
    VkPipelineLayoutCreateInfo li = {.sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                                     .setLayoutCount = 1,
                                     .pSetLayouts = &M.cdsl,
                                     .pushConstantRangeCount = 1,
                                     .pPushConstantRanges = &pr};
    if (vkCreatePipelineLayout(c->device, &li, NULL, &M.cpl) != VK_SUCCESS ||
        vkCreateShaderModule(c->device, &mi, NULL, &m) != VK_SUCCESS)
        return 0;
    VkComputePipelineCreateInfo ci = {.sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
                                      .stage = {.sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                                                .stage = VK_SHADER_STAGE_COMPUTE_BIT,
                                                .module = m,
                                                .pName = "main"},
                                      .layout = M.cpl};
    VkResult r = vkCreateComputePipelines(c->device, VK_NULL_HANDLE, 1, &ci, NULL, &M.cpipe);
    vkDestroyShaderModule(c->device, m, NULL);
    return r == VK_SUCCESS;
}

static int ensure_color(uint32_t w, uint32_t h) {
    if (M.col && M.cw == w && M.ch == h) return 1;
    const VkCtx *c = vk_ctx();
    if (M.col_view) vkDestroyImageView(c->device, M.col_view, NULL), M.col_view = VK_NULL_HANDLE;
    if (M.col) vmaDestroyImage(c->vma, M.col, M.col_mem), M.col = VK_NULL_HANDLE;
    if (!make_image(w, h, VK_FORMAT_R16G16B16A16_SFLOAT,
                    VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_SAMPLED_BIT, &M.col, &M.col_mem, &M.col_view))
        return 0;
    M.cw = w, M.ch = h;
    return 1;
}

static int ensure_src_view(VkImage img, VkFormat fmt) {
    if (M.src_view && M.src == img && M.src_fmt == fmt) return 1;
    const VkCtx *c = vk_ctx();
    if (M.src_view) vkDestroyImageView(c->device, M.src_view, NULL), M.src_view = VK_NULL_HANDLE;
    VkImageViewCreateInfo vi = {.sType = VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                                .image = img,
                                .viewType = VK_IMAGE_VIEW_TYPE_2D,
                                .format = fmt,
                                .subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1}};
    if (vkCreateImageView(c->device, &vi, NULL, &M.src_view) != VK_SUCCESS) return 0;
    M.src = img, M.src_fmt = fmt;
    return 1;
}

// FSR 4 wants an exposure resource; the game's picture is tonemapped, so 1.0 is the value.
static int ensure_exposure(VkCommandBuffer cb) {
    if (!M.exp) {
        if (!make_image(1, 1, VK_FORMAT_R32_SFLOAT,
                        VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT, &M.exp, &M.exp_mem, &(VkImageView){0}))
            return 0;
        VkImageMemoryBarrier2 b = {.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2,
                                   .srcStageMask = VK_PIPELINE_STAGE_2_NONE,
                                   .dstStageMask = VK_PIPELINE_STAGE_2_CLEAR_BIT | VK_PIPELINE_STAGE_2_TRANSFER_BIT,
                                   .dstAccessMask = VK_ACCESS_2_TRANSFER_WRITE_BIT,
                                   .oldLayout = VK_IMAGE_LAYOUT_GENERAL,
                                   .newLayout = VK_IMAGE_LAYOUT_GENERAL,
                                   .image = M.exp,
                                   .subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1}};
        VkDependencyInfo d = {.sType = VK_STRUCTURE_TYPE_DEPENDENCY_INFO, .imageMemoryBarrierCount = 1,
                              .pImageMemoryBarriers = &b};
        vkCmdPipelineBarrier2(cb, &d);
    }
    if (!M.exp_ready) {
        VkClearColorValue v = {.float32 = {1.0f, 0.0f, 0.0f, 0.0f}};
        VkImageSubresourceRange r = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
        vkCmdClearColorImage(cb, M.exp, VK_IMAGE_LAYOUT_GENERAL, &v, 1, &r);
        full_barrier(cb);
        M.exp_ready = 1;
    }
    return 1;
}

// Converts the game's picture to the linear RGBA16F colour the FSR provider takes. 0: no conversion pipeline.
static int convert_color(VkCommandBuffer cb, VkImage src, VkFormat fmt, uint32_t w, uint32_t h) {
    if (!color_init() || !ensure_color(w, h) || !ensure_src_view(src, fmt)) return 0;
    VkDescriptorImageInfo si = {M.samp, M.src_view, VK_IMAGE_LAYOUT_GENERAL};
    VkDescriptorImageInfo di = {VK_NULL_HANDLE, M.col_view, VK_IMAGE_LAYOUT_GENERAL};
    VkWriteDescriptorSet wr[2] = {
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 0, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, .pImageInfo = &si},
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 1, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, .pImageInfo = &di}};
    vkCmdBindPipeline(cb, VK_PIPELINE_BIND_POINT_COMPUTE, M.cpipe);
    M.push(cb, VK_PIPELINE_BIND_POINT_COMPUTE, M.cpl, 0, 2, wr);
    float pc[4] = {(float)w, (float)h, fmt == VK_FORMAT_R8G8B8A8_SRGB || fmt == VK_FORMAT_B8G8R8A8_SRGB ? 1.0f : 0.0f, 0.0f};
    vkCmdPushConstants(cb, M.cpl, VK_SHADER_STAGE_COMPUTE_BIT, 0, sizeof pc, pc);
    vkCmdDispatch(cb, (w + 7) / 8, (h + 7) / 8, 1);
    full_barrier(cb);
    return 1;
}

static int mv_init(void) {
    if (M.ok) return 1;
    const VkCtx *c = vk_ctx();
    M.push = (PFN_vkCmdPushDescriptorSetKHR)vkGetDeviceProcAddr(c->device, "vkCmdPushDescriptorSetKHR");
    if (!M.push) return 0;
    VkDescriptorSetLayoutBinding b[8] = {
        {0, VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL},
        {1, VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL},
        {2, VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL},
        {3, VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL},
        {4, VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL},
        {5, VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL},
        {6, VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL},
        {7, VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, 1, VK_SHADER_STAGE_COMPUTE_BIT, NULL}};
    VkDescriptorSetLayoutCreateInfo dci = {.sType = VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
                                           .flags = VK_DESCRIPTOR_SET_LAYOUT_CREATE_PUSH_DESCRIPTOR_BIT_KHR,
                                           .bindingCount = 8, .pBindings = b};
    VkPushConstantRange pr = {VK_SHADER_STAGE_COMPUTE_BIT, 0, 24};
    VkSamplerCreateInfo si = {.sType = VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO,
                              .magFilter = VK_FILTER_NEAREST,
                              .minFilter = VK_FILTER_NEAREST,
                              .mipmapMode = VK_SAMPLER_MIPMAP_MODE_NEAREST,
                              .addressModeU = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                              .addressModeV = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                              .addressModeW = VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE};
    VkShaderModuleCreateInfo mi = {.sType = VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
                                   .codeSize = sizeof dlaa_mv_spv,
                                   .pCode = dlaa_mv_spv};
    VkShaderModule m = VK_NULL_HANDLE;
    VkBufferCreateInfo bi = {.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
                             .size = 512,
                             .usage = VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT};
    VmaAllocationCreateInfo ai = {.usage = VMA_MEMORY_USAGE_AUTO_PREFER_HOST,
                                  .flags = VMA_ALLOCATION_CREATE_HOST_ACCESS_SEQUENTIAL_WRITE_BIT |
                                           VMA_ALLOCATION_CREATE_MAPPED_BIT};
    VmaAllocationInfo info;
    if (vkCreateDescriptorSetLayout(c->device, &dci, NULL, &M.dsl) != VK_SUCCESS) return 0;
    VkPipelineLayoutCreateInfo li = {.sType = VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                                     .setLayoutCount = 1,
                                     .pSetLayouts = &M.dsl,
                                     .pushConstantRangeCount = 1,
                                     .pPushConstantRanges = &pr};
    if (vkCreatePipelineLayout(c->device, &li, NULL, &M.pl) != VK_SUCCESS ||
        vkCreateSampler(c->device, &si, NULL, &M.samp) != VK_SUCCESS ||
        vkCreateShaderModule(c->device, &mi, NULL, &m) != VK_SUCCESS ||
        vmaCreateBuffer(c->vma, &bi, &ai, &M.ubo, &M.ubo_mem, &info) != VK_SUCCESS)
        return 0;
    M.ubo_map = info.pMappedData;
    VkComputePipelineCreateInfo ci = {.sType = VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO,
                                      .stage = {.sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                                                .stage = VK_SHADER_STAGE_COMPUTE_BIT,
                                                .module = m,
                                                .pName = "main"},
                                      .layout = M.pl};
    VkResult r = vkCreateComputePipelines(c->device, VK_NULL_HANDLE, 1, &ci, NULL, &M.pipe);
    vkDestroyShaderModule(c->device, m, NULL);
    if (r != VK_SUCCESS) return 0;
    // The mask bindings are unused with bias 0; one small R8 image stands in for them.
    if (!make_image(2, 2, VK_FORMAT_R8_UNORM, VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_SAMPLED_BIT, &M.dummy,
                    &M.dummy_mem, &M.dummy_view))
        return 0;
    M.ok = 1;
    return 1;
}

// This frame's camera motion vectors at w x h into M.mv, from the scene depth and the view-projection; 0: no history.
static int mv_pass(VkCommandBuffer cb, uint32_t w, uint32_t h, float jx, float jy, const float vp[16]) {
    if (!mv_init()) return 0;
    if (!M.mv || M.mv_view == VK_NULL_HANDLE || M.mw != w || M.mh != h) {
        if (M.mv_view) vkDestroyImageView(vk_ctx()->device, M.mv_view, NULL);
        if (M.mv) vmaDestroyImage(vk_ctx()->vma, M.mv, M.mv_mem), M.mv = VK_NULL_HANDLE;
        if (!make_image(w, h, VK_FORMAT_R16G16_SFLOAT, VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_SAMPLED_BIT, &M.mv,
                        &M.mv_mem, &M.mv_view))
            return 0;
        M.mw = w, M.mh = h, M.resized = 1;
    }
    if (!M.depth_view) return 0;
    double cur[16], inv[16], m[16];
    for (int i = 0; i < 16; i++) cur[i] = vp[i];
    if (!M.prev_ok) {
        memcpy(M.prev_vp, cur, sizeof cur);
        memcpy(M.vp, cur, sizeof cur);
        M.prev_ok = 1;
        return 0;
    }
    if (!inv4(cur, inv)) return 0;
    for (int r = 0; r < 4; r++)
        for (int k = 0; k < 4; k++) {
            double v = 0;
            for (int j = 0; j < 4; j++) v += M.prev_vp[r * 4 + j] * inv[j * 4 + k];
            m[r * 4 + k] = v;
        }
    memcpy(M.vp, cur, sizeof cur);   // committed after the provider takes the frame (fsr4_commit)
    float *blk = (float *)M.ubo_map;
    memset(blk, 0, 512);
    for (int r = 0; r < 4; r++)
        for (int k = 0; k < 4; k++) blk[12 + r * 4 + k] = r == k ? 1.0f : 0.0f,
                                         blk[64 + 12 + r * 4 + k] = (float)m[r * 4 + k];
    vmaFlushAllocation(vk_ctx()->vma, M.ubo_mem, 0, 512);
    VkDescriptorImageInfo zi = {M.samp, M.depth_view, VK_IMAGE_LAYOUT_GENERAL};
    VkDescriptorImageInfo oi = {VK_NULL_HANDLE, M.mv_view, VK_IMAGE_LAYOUT_GENERAL};
    VkDescriptorBufferInfo cur_bi = {M.ubo, 0, 256};
    VkDescriptorBufferInfo prev_bi = {M.ubo, 256, 256};
    VkDescriptorImageInfo di = {M.samp, M.dummy_view, VK_IMAGE_LAYOUT_GENERAL};
    VkDescriptorImageInfo dsi = {VK_NULL_HANDLE, M.dummy_view, VK_IMAGE_LAYOUT_GENERAL};
    VkWriteDescriptorSet wr[8] = {
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 0, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, .pImageInfo = &zi},
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 1, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, .pImageInfo = &oi},
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 2, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, .pBufferInfo = &cur_bi},
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 3, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER, .pBufferInfo = &prev_bi},
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 4, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER, .pImageInfo = &di},
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 5, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, .pImageInfo = &dsi},
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 6, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, .pImageInfo = &dsi},
        {.sType = VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET, .dstBinding = 7, .descriptorCount = 1,
         .descriptorType = VK_DESCRIPTOR_TYPE_STORAGE_IMAGE, .pImageInfo = &dsi}};
    vkCmdBindPipeline(cb, VK_PIPELINE_BIND_POINT_COMPUTE, M.pipe);
    M.push(cb, VK_PIPELINE_BIND_POINT_COMPUTE, M.pl, 0, 8, wr);
    float pc[6] = {(float)w, (float)h, jx, jy, 0.0f, 0.0f};
    vkCmdPushConstants(cb, M.pl, VK_SHADER_STAGE_COMPUTE_BIT, 0, sizeof pc, pc);
    vkCmdDispatch(cb, (w + 7) / 8, (h + 7) / 8, 1);
    full_barrier(cb);
    return 1;
}

static uint32_t ffx_format(VkFormat f) {
    switch (f) {
    case VK_FORMAT_R8G8B8A8_UNORM: return FFX_API_SURFACE_FORMAT_R8G8B8A8_UNORM;
    case VK_FORMAT_R8G8B8A8_SRGB: return FFX_API_SURFACE_FORMAT_R8G8B8A8_SRGB;
    case VK_FORMAT_B8G8R8A8_UNORM: return FFX_API_SURFACE_FORMAT_B8G8R8A8_UNORM;
    case VK_FORMAT_B8G8R8A8_SRGB: return FFX_API_SURFACE_FORMAT_B8G8R8A8_SRGB;
    case VK_FORMAT_R16G16B16A16_SFLOAT: return FFX_API_SURFACE_FORMAT_R16G16B16A16_FLOAT;
    case VK_FORMAT_R16G16_SFLOAT: return FFX_API_SURFACE_FORMAT_R16G16_FLOAT;
    case VK_FORMAT_B10G11R11_UFLOAT_PACK32: return FFX_API_SURFACE_FORMAT_R11G11B10_FLOAT;
    case VK_FORMAT_A2B10G10R10_UNORM_PACK32: return FFX_API_SURFACE_FORMAT_R10G10B10A2_UNORM;
    case VK_FORMAT_D32_SFLOAT: return FFX_API_SURFACE_FORMAT_R32_FLOAT;
    case VK_FORMAT_D16_UNORM: return FFX_API_SURFACE_FORMAT_R16_UNORM;
    case VK_FORMAT_R32_SFLOAT: return FFX_API_SURFACE_FORMAT_R32_FLOAT;
    default: return FFX_API_SURFACE_FORMAT_UNKNOWN;
    }
}

static struct FfxApiResource resource(VkImage img, VkFormat fmt, uint32_t w, uint32_t h, uint32_t usage, uint32_t state) {
    struct FfxApiResource r = {0};
    r.resource = (void *)img;
    r.description.type = FFX_API_RESOURCE_TYPE_TEXTURE2D;
    r.description.format = ffx_format(fmt);
    r.description.width = w;
    r.description.height = h;
    r.description.depth = 1;
    r.description.mipCount = 1;
    r.description.flags = 0;
    r.description.usage = usage;
    r.state = state;
    return r;
}

static int ensure_output(uint32_t w, uint32_t h) {
    if (M.out && M.ow == w && M.oh == h) return 1;
    M.resized = 1;
    const VkCtx *c = vk_ctx();
    if (M.out_view) vkDestroyImageView(c->device, M.out_view, NULL), M.out_view = VK_NULL_HANDLE;
    if (M.out) vmaDestroyImage(c->vma, M.out, M.out_mem), M.out = VK_NULL_HANDLE;
    if (!make_image(w, h, VK_FORMAT_R16G16B16A16_SFLOAT,
                    VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT, &M.out,
                    &M.out_mem, &M.out_view))
        return 0;
    M.ow = w, M.oh = h;
    return 1;
}

// One FSR frame: motion vectors from the scene depth, then the provider's upscale of the game's picture into an image
// the size of the destination rectangle, blitted into dst (left in TRANSFER_DST). 0: not handled (the caller uses FSR 1
// or the bilinear blit).
int vk_fsr4_record(VkCommandBuffer cb, VkImage src, VkFormat fmt, int sw, int sh, const int crop[4], VkImage dst,
                   const int to[4]) {
    if (!vk_fsr4_ready() || !mv_init()) return 0;
    VkImage depth;
    VkFormat dfmt;
    uint32_t dw = 0, dh = 0;
    float vp[16], jx = 0, jy = 0, near = 0.2f, far = 2000.0f, fov = 0.9599f;
    if (!vk_dlaa_scene_data(&depth, &dfmt, &dw, &dh, vp, &jx, &jy, &near, &far, &fov)) return 0;
    if ((int)dw != sw || (int)dh != sh) return 0;                       // the scene must match the picture's size
    if (crop && (crop[0] || crop[1] || crop[2] != sw || crop[3] != sh)) return 0;   // a crop: not implemented yet
    if (!ensure_depth_view(depth, dfmt)) return 0;
    if (!mv_pass(cb, (uint32_t)sw, (uint32_t)sh, jx, jy, vp)) return 0;   // no history yet: FSR 1 this frame
    if (!convert_color(cb, src, fmt, (uint32_t)sw, (uint32_t)sh)) return 0;
    if (!ensure_exposure(cb)) return 0;
    if (!ensure_output((uint32_t)to[2], (uint32_t)to[3])) return 0;
    static int have_frame;
    // a frame the provider did not take (crop, no scene, a size change) leaves its history behind this one: start over
    uint64_t frame = gfx_stats().frames;
    if (M.resized) M.resized = 0, have_frame = 0;
    if (have_frame && frame != M.last_frame + 1 && frame != M.last_frame) have_frame = 0;
    struct ffxDispatchDescUpscale d = {0};
    d.header.type = FFX_API_DISPATCH_DESC_TYPE_UPSCALE;
    d.commandList = cb;
    d.color = resource(M.col, VK_FORMAT_R16G16B16A16_SFLOAT, (uint32_t)sw, (uint32_t)sh,
                       FFX_API_RESOURCE_USAGE_READ_ONLY, FFX_API_RESOURCE_STATE_COMPUTE_READ);
    d.depth = resource(depth, dfmt, dw, dh, FFX_API_RESOURCE_USAGE_READ_ONLY,
                       FFX_API_RESOURCE_STATE_COMPUTE_READ);
    d.motionVectors = resource(M.mv, VK_FORMAT_R16G16_SFLOAT, (uint32_t)sw, (uint32_t)sh,
                               FFX_API_RESOURCE_USAGE_READ_ONLY, FFX_API_RESOURCE_STATE_COMPUTE_READ);
    d.output = resource(M.out, VK_FORMAT_R16G16B16A16_SFLOAT, (uint32_t)to[2], (uint32_t)to[3],
                        FFX_API_RESOURCE_USAGE_UAV, FFX_API_RESOURCE_STATE_UNORDERED_ACCESS);
    d.exposure = resource(M.exp, VK_FORMAT_R32_SFLOAT, 1, 1, FFX_API_RESOURCE_USAGE_READ_ONLY,
                          FFX_API_RESOURCE_STATE_COMPUTE_READ);
    d.jitterOffset = (struct FfxApiFloatCoords2D){jx, jy};
    d.motionVectorScale = (struct FfxApiFloatCoords2D){1.0f, 1.0f};
    d.renderSize = (struct FfxApiDimensions2D){(uint32_t)sw, (uint32_t)sh};
    d.upscaleSize = (struct FfxApiDimensions2D){(uint32_t)to[2], (uint32_t)to[3]};
    // the same TOTK_SHARPNESS the FSR 1 path's RCAS takes (0..100, 80 by default); the provider's 0..1 is the same
    // direction: 0 leaves its picture as the accumulation left it, 1 is the most it sharpens
    int sharp = cfg_int("TOTK_SHARPNESS");
    if (sharp < 0) sharp = 0;
    if (sharp > 100) sharp = 100;
    d.enableSharpening = sharp > 0;
    d.sharpness = sharp / 100.0f;
    d.frameTimeDelta = 1000.0f / 60.0f;
    d.preExposure = 1.0f;
    d.reset = !have_frame;
    d.cameraNear = near;   // the game's own, from its camera block (Splatoon's plaza: 0.2 and 2000, 55 degrees)
    d.cameraFar = far;
    d.cameraFovAngleVertical = fov;
    d.viewSpaceToMetersFactor = 1.0f;
    ffxReturnCode_t r = U.dispatch(&U.ctx, &d.header);
    if (r != FFX_API_RETURN_OK) {
        if (!M.logged) rt_log("fsr4: ffxDispatch failed (%u); upscaler stays FSR 1", (unsigned)r), M.logged = 1;
        return 0;
    }
    have_frame = 1;
    memcpy(M.prev_vp, M.vp, sizeof M.prev_vp);
    M.last_frame = gfx_stats().frames;
    {
        static uint64_t ok;
        ok++;
        if (ok == 1) rt_log("fsr4: first frame upscaled %dx%d -> %dx%d", sw, sh, to[2], to[3]);
        else if (ok % 300 == 0) rt_log("fsr4: %llu frames upscaled", (unsigned long long)ok);
    }
    VkImageMemoryBarrier2 b = {.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER_2,
                               .srcStageMask = VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT,
                               .srcAccessMask = VK_ACCESS_2_SHADER_WRITE_BIT,
                               .dstStageMask = VK_PIPELINE_STAGE_2_TRANSFER_BIT,
                               .dstAccessMask = VK_ACCESS_2_TRANSFER_READ_BIT,
                               .oldLayout = VK_IMAGE_LAYOUT_GENERAL,
                               .newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                               .image = M.out,
                               .subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1}};
    VkDependencyInfo dep = {.sType = VK_STRUCTURE_TYPE_DEPENDENCY_INFO, .imageMemoryBarrierCount = 1,
                            .pImageMemoryBarriers = &b};
    vkCmdPipelineBarrier2(cb, &dep);
    VkImageBlit region = {.srcSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1},
                          .srcOffsets = {{0, 0, 0}, {(int32_t)to[2], (int32_t)to[3], 1}},
                          .dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1},
                          .dstOffsets = {{to[0], to[1], 0}, {to[0] + to[2], to[1] + to[3], 1}}};
    vkCmdBlitImage(cb, M.out, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, dst, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region,
                   VK_FILTER_LINEAR);
    b.srcStageMask = VK_PIPELINE_STAGE_2_TRANSFER_BIT;
    b.srcAccessMask = VK_ACCESS_2_TRANSFER_READ_BIT;
    b.dstStageMask = VK_PIPELINE_STAGE_2_COMPUTE_SHADER_BIT;
    b.dstAccessMask = VK_ACCESS_2_SHADER_READ_BIT | VK_ACCESS_2_SHADER_WRITE_BIT;
    b.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
    b.newLayout = VK_IMAGE_LAYOUT_GENERAL;
    vkCmdPipelineBarrier2(cb, &dep);
    return 1;
}
