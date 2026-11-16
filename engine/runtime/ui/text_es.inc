// Spanish (neutral) text of the menu and of its settings drawer (ui_text.h UI_MENU_FIRST..UI_MENU_LAST_ID), included into
// text_es. Every menu id has its text: the check in ov_probe.c fails when one is missing. Proper names (Ultrahand,
// TKMM, Atmosphere, DLSS, FSR...) and printf format specifiers stay as they are.
    [UI_MENU_TITLE] = "Ajustes",
    [UI_MENU_TAB_SCREEN] = "Pantalla", [UI_MENU_TAB_GAME] = "Juego", [UI_MENU_TAB_MODS] = "Mods", [UI_MENU_TAB_SOUND] = "Sonido",
    [UI_MENU_TAB_CONTROLS] = "Controles", [UI_MENU_TAB_AMIIBO] = "Amiibo", [UI_MENU_TAB_MENU] = "Menú", [UI_MENU_TAB_ADVANCED] = "Avanzado",
    [UI_MENU_CONTINUE] = "Continuar", [UI_MENU_QUIT] = "Salir del juego",
    [UI_MENU_ON] = "Sí", [UI_MENU_OFF] = "No", [UI_MENU_AUTO] = "Automático", [UI_MENU_DEFAULT_MARK] = "predeterminado",
    [UI_MENU_AFTER_RESTART] = "tras reiniciar", [UI_MENU_SET_BY_ENV] = "definido por una variable de entorno",
    [UI_MENU_SAVE_FAILED] = "no se pudieron guardar los ajustes",
    [UI_MENU_VERSION] = "%s %s, juego %s",

    [UI_MENU_FULLSCREEN] = "Pantalla completa", [UI_MENU_FULLSCREEN_DESC] = "El juego ocupa toda la pantalla; F11 lo alterna.",
    [UI_MENU_WINDOW_SIZE] = "Tamaño de la ventana", [UI_MENU_WINDOW_SIZE_DESC] = "Tamaño de la ventana del juego cuando no está a pantalla completa.",
    [UI_MENU_WINDOW_SIZE_CUSTOM] = "Personalizado",
    [UI_MENU_PRESENT] = "Presentación", [UI_MENU_PRESENT_DESC] = "Cómo llegan a la pantalla los fotogramas terminados.",
    [UI_MENU_PRESENT_MAILBOX] = "Mailbox", [UI_MENU_PRESENT_VSYNC] = "VSync", [UI_MENU_PRESENT_IMMEDIATE] = "Inmediata",
    [UI_MENU_PRESENT_IN_USE] = "en uso: %s",
    [UI_MENU_GPU] = "Tarjeta gráfica", [UI_MENU_GPU_DESC] = "El dispositivo Vulkan que dibuja el juego.",
    [UI_MENU_GPU_AUTO] = "Automática",
    [UI_MENU_PERF_OVERLAY] = "Contador de rendimiento",
    [UI_MENU_PERF_OVERLAY_DESC] = "Muestra los fotogramas por segundo en la esquina superior derecha. Completo añade los tiempos de "
                                  "fotograma con una gráfica, la carga de CPU, GPU y memoria, las temperaturas y las esperas del "
                                  "hilo de renderizado. F10 cambia de modo.",
    [UI_MENU_PERF_OFF] = "No", [UI_MENU_PERF_FPS] = "FPS", [UI_MENU_PERF_FULL] = "Completo",
    // printf format of the counter's line in FPS mode: the frame rate
    [UI_PERF_LINE_FPS] = "%.0f fps",
    [UI_MENU_GRAPHICS] = "Imagen",
    [UI_MENU_OUTPUT_RES] = "Resolución del juego",
    [UI_MENU_OUTPUT_RES_DESC] = "El tamaño con el que el juego dibuja su imagen y su interfaz. Por encima de 1920 x 1080 el juego "
                                "necesita más memoria de vídeo y más tiempo de GPU. Un tamaño más ancho (21:9) amplía el campo "
                                "de visión de la cámara y lleva la interfaz a las esquinas de la pantalla; uno más alto (16:10, "
                                "3:2) la lleva a los bordes superior e inferior.",
    [UI_MENU_RENDER_SCALE] = "Escala de renderizado 3D",
    [UI_MENU_RENDER_SCALE_DESC] = "Tamaño de la escena 3D respecto a la resolución del juego; el FSR del juego la escala a ese "
                                  "tamaño. Por encima del 100 % suaviza los bordes dentados (supermuestreo) con un coste alto de GPU.",
    [UI_MENU_RENDER_SCALE_GAME] = "Juego (83 %)",
    [UI_MENU_DYNAMIC_RES] = "Resolución dinámica",
    [UI_MENU_DYNAMIC_RES_DESC] = "Permite que el juego reduzca la escena 3D al 90 o al 80 %. Lo decide con una estimación hecha para la Switch que ignora tu GPU, así que las escenas amplias bajan incluso en un PC rápido. No mantiene la escala de renderizado.",
    [UI_MENU_UPSCALER] = "Escalado a la ventana",
    [UI_MENU_UPSCALER_DESC] = "Cómo se escala la imagen cuando la ventana es mayor que la resolución del juego. FSR 4.1 y 3.1 necesitan el runtime FidelityFX de AMD junto al ejecutable. FSR conserva los "
                              "bordes y el detalle con más nitidez que el bilineal.",
    [UI_MENU_UPSCALER_FSR4] = "AMD FSR 4.1", [UI_MENU_UPSCALER_FSR3] = "AMD FSR 3.1",
    [UI_MENU_UPSCALER_FSR] = "AMD FSR 1",
    [UI_MENU_UPSCALER_BILINEAR] = "Bilineal",
    [UI_MENU_ASPECT] = "Forma de la imagen",
    [UI_MENU_ASPECT_DESC] = "Cuando la forma de la ventana difiere de la de la resolución del juego (una imagen 16:9 en una "
                            "pantalla 21:9): mantener la forma de la imagen con bandas negras, o estirarla por toda la ventana.",
    [UI_MENU_ASPECT_FIT] = "Bandas negras", [UI_MENU_ASPECT_STRETCH] = "Estirar",
    [UI_MENU_MOVIE_ASPECT] = "Forma de los vídeos",
    [UI_MENU_MOVIE_ASPECT_DESC] = "Cuando la resolución del juego no es 16:9: mantener la forma propia de los vídeos de las "
                                  "escenas con bandas negras, o estirarlos por toda la imagen. Se aplica desde el siguiente vídeo.",
    [UI_MENU_MOVIE_ASPECT_KEEP] = "Bandas negras", [UI_MENU_MOVIE_ASPECT_STRETCH] = "Estirar",
    [UI_MENU_FOV] = "Campo de visión",
    [UI_MENU_FOV_DESC] = "Cuánto del mundo muestra la cámara, en porcentaje del propio del juego. Solo en la versión 1.4.3 del juego.",
    [UI_MENU_SHADOW_RES] = "Tamaño del mapa de sombras",
    [UI_MENU_SHADOW_RES_DESC] = "Tamaño del mapa de sombras del sol. El tamaño propio del juego cambia con la vista, hasta unos 400 "
                                "cerca del suelo, de modo que las sombras de las hojas se ven pixeladas y saltan; un tamaño fijo "
                                "las mantiene quietas y más nítidas. Por encima de 1024 las sombras cuestan más tiempo de GPU y "
                                "memoria de vídeo; 256 es más tosco que el del juego y es para GPU integradas con poco ancho de "
                                "banda de memoria.",
    [UI_MENU_SHADOW_RES_GAME] = "Juego (cambia)",
    [UI_MENU_ANISO] = "Filtrado de texturas",
    [UI_MENU_ANISO_DESC] = "Filtrado anisótropo de las texturas de superficie: el suelo, los caminos y las paredes vistos en ángulo rasante se mantienen nítidos a lo lejos en vez de desenfocarse. Juego: el nivel que pide el juego, como en la Switch.",
    [UI_MENU_ANISO_GAME] = "Juego",
    [UI_MENU_SHARPNESS] = "Nitidez de FSR",
    [UI_MENU_SHARPNESS_DESC] = "Enfoque tras el escalado de FSR; 0 lo desactiva.",
    [UI_MENU_COLOR] = "Color",
    [UI_MENU_COLOR_PRESET] = "Perfil de color",
    [UI_MENU_COLOR_PRESET_DESC] = "Ajusta los cuatro valores siguientes. Intenso: más contraste y saturación, con las sombras algo "
                                  "abiertas (pensado para pantallas OLED). Claridad: un poco más de contraste, saturación y "
                                  "brillo. Al mover un valor, el perfil pasa a Personalizado. La interfaz del juego cambia con la "
                                  "imagen; este menú no.",
    [UI_MENU_COLOR_DEFAULT] = "El del juego", [UI_MENU_COLOR_VIVID] = "Intenso (OLED)", [UI_MENU_COLOR_CLARITY] = "Claridad",
    [UI_MENU_COLOR_CUSTOM] = "Personalizado",
    [UI_MENU_COLOR_BRIGHTNESS] = "Brillo", [UI_MENU_COLOR_BRIGHTNESS_DESC] = "Se suma a cada color; 0 lo deja como está.",
    [UI_MENU_COLOR_CONTRAST] = "Contraste",
    [UI_MENU_COLOR_CONTRAST_DESC] = "Aleja los colores del gris medio (por encima del 100 %) o los acerca a él; 100 % los deja como están.",
    [UI_MENU_COLOR_SATURATION] = "Saturación",
    [UI_MENU_COLOR_SATURATION_DESC] = "Intensidad del color frente al gris; 100 % la deja como está.",
    [UI_MENU_COLOR_GAMMA] = "Gamma",
    [UI_MENU_COLOR_GAMMA_DESC] = "Por debajo de 1 aclara los tonos oscuros y medios, por encima de 1 los oscurece; 1 los deja como están.",
    [UI_MENU_HDR_GROUP] = "HDR",
    [UI_MENU_HDR] = "Salida HDR",
    [UI_MENU_HDR_DESC] = "Altas luces brillantes en una pantalla HDR. Requiere tener el HDR activado en Windows (\"Usar HDR\") o en "
                         "un compositor de Wayland con gestión del color; si no, la imagen sigue en SDR. Se aplica tras "
                         "reiniciar. El perfil de color de arriba es para SDR y no se usa en HDR.",
    [UI_MENU_HDR_PEAK] = "Brillo máximo",
    [UI_MENU_HDR_PEAK_DESC] = "El pico de tu pantalla en nits; las altas luces más brillantes se detienen ahí.",
    [UI_MENU_HDR_GAME] = "Brillo del juego",
    [UI_MENU_HDR_GAME_DESC] = "Nits del blanco de la imagen (blanco papel). 203 es la referencia HDR; súbelo en una habitación "
                              "luminosa.",
    [UI_MENU_HDR_UI] = "Brillo de la interfaz y del menú",
    [UI_MENU_HDR_UI_DESC] = "Nits del blanco de la interfaz del juego y de este menú.",
    [UI_MENU_HDR_HUE] = "Corrección del tono de las altas luces",
    [UI_MENU_HDR_HUE_DESC] = "Los colores brillantes, como el fuego y las chispas, tiran hacia el blanco igual que en la imagen SDR "
                             "del propio juego, a juego con la luz que proyectan; 0 los mantiene totalmente saturados.",
    [UI_MENU_SHADOW_SCALE] = "Resolución de las sombras",
    [UI_MENU_SHADOW_SCALE_DESC] = "Dibuja los mapas de sombras del sol tantas veces más grandes por lado que los 960x960 del juego: "
                                  "bordes de sombra más nítidos y sombras de hojas más definidas. Se aplica en el próximo inicio.",
    [UI_MENU_PCSS] = "Sombras suaves",
    [UI_MENU_PCSS_DESC] = "Las sombras del sol son nítidas donde un objeto toca el suelo y más suaves cuanto más lejos cae la "
                          "sombra (PCSS). 0 mantiene el filtro del juego. Se aplica en el próximo inicio.",
    [UI_MENU_CONTACT_SHADOWS] = "Sombras de contacto",
    [UI_MENU_CONTACT_SHADOWS_DESC] = "Pequeñas sombras del sol que los mapas de sombras no captan: en los pies, bajo las piedras, en "
                                     "la hierba y en los pliegues. Se trazan sobre la pantalla tras la iluminación. Se aplica en "
                                     "el próximo inicio.",
    [UI_MENU_SHADOW_RANGE] = "Alcance de las cascadas de sombras",
    [UI_MENU_SHADOW_RANGE_DESC] = "Aleja los escalones donde las sombras se suavizan (los 20, 40, 80 y 160 m del juego). Reparte el "
                                  "mapa de sombras por más terreno: sube la resolución de las sombras a la vez. Se aplica en el "
                                  "próximo inicio.",
    [UI_MENU_DLAA] = "NVIDIA DLSS / DLAA",
    [UI_MENU_DLAA_DESC] = "NVIDIA DLSS para la escena 3D (GPU NVIDIA RTX; " DLSS_FILE " junto a " EXE_FILE "): bordes más suaves y "
                          "menos centelleo. La interfaz del juego no se toca. El modo de abajo fija la resolución de la que "
                          "parte. Se aplica tras reiniciar.",
    [UI_MENU_DLSS_PRESET] = "Modelo de DLSS",
    [UI_MENU_DLSS_PRESET_DESC] = "Qué modelo de DLSS se ejecuta. Automático sigue la recomendación de NVIDIA por modo: K para DLAA, "
                                 "Calidad y Equilibrado, M para Rendimiento, L para Rendimiento extremo. J: algo menos de estela "
                                 "tras los personajes en movimiento, algo más de parpadeo. K: el modelo más nítido en todos los "
                                 "modos. Se aplica al instante.",
    [UI_MENU_DLSS_PRESET_DEFAULT] = "Automático", [UI_MENU_DLSS_PRESET_K] = "K (el más nítido)",
    [UI_MENU_DLSS_PRESET_J] = "J (menos estela)",
    [UI_MENU_DLSS_CHARS] = "Estelas de personajes con DLSS",
    [UI_MENU_DLSS_CHARS_DESC] = "Reduce las estelas tenues tras los personajes y animales en movimiento y lo que sostienen: DLSS "
                                "aprovecha menos los fotogramas anteriores en esas zonas. Más fuerte quita más estelas y deja "
                                "esos bordes algo menos suaves. Se aplica al instante.",
    [UI_MENU_DLSS_CHARS_OFF] = "No", [UI_MENU_DLSS_CHARS_LIGHT] = "Leve", [UI_MENU_DLSS_CHARS_MEDIUM] = "Medio",
    [UI_MENU_DLSS_CHARS_FULL] = "Máximo",
    [UI_MENU_DLSS] = "Modo DLSS",
    [UI_MENU_DLSS_DESC] = "DLAA dibuja la escena 3D a resolución completa. Calidad, Equilibrado, Rendimiento y Rendimiento extremo "
                          "la dibujan al 67, 58, 50 y 33 % y dejan que DLSS la amplíe, lo que ahorra tiempo de GPU; sustituyen a "
                          "la escala de renderizado 3D. Se aplica al instante.",
    [UI_MENU_DLSS_DLAA] = "DLAA (nativo)", [UI_MENU_DLSS_QUALITY] = "Calidad", [UI_MENU_DLSS_BALANCED] = "Equilibrado",
    [UI_MENU_DLSS_PERFORMANCE] = "Rendimiento", [UI_MENU_DLSS_ULTRA] = "Rendimiento extremo",
    [UI_MENU_FXAA] = "Suavizado de bordes del juego (FXAA)",
    [UI_MENU_FXAA_DESC] = "El suavizado de bordes FXAA propio del juego. Difumina un poco el detalle fino; con DLAA activado no hace falta.",

    [UI_MENU_FPS_LIMIT] = "Límite de fotogramas",
    [UI_MENU_FPS_LIMIT_DESC] = "El máximo de fotogramas por segundo que dibuja el juego. Por encima de 60 es experimental y "
                               "requiere que la velocidad del juego siga el reloj.",
    [UI_MENU_FPS_30] = "30 FPS", [UI_MENU_FPS_40] = "40 FPS", [UI_MENU_FPS_45] = "45 FPS",
    [UI_MENU_FPS_60] = "60 FPS", [UI_MENU_FPS_90] = "90 FPS", [UI_MENU_FPS_120] = "120 FPS", [UI_MENU_FPS_144] = "144 FPS",
    // the monitor's refresh rate; a printf format in Hz when the value is known
    [UI_MENU_FPS_DISPLAY] = "Frecuencia del monitor", [UI_MENU_FPS_DISPLAY_HZ] = "Frecuencia del monitor (%.0f Hz)",
    [UI_MENU_FRAME_QUEUE] = "Cola de fotogramas",
    [UI_MENU_FRAME_QUEUE_DESC] = "El juego empieza un fotograma mientras el anterior espera su turno en pantalla: muchos menos "
                                 "fotogramas perdidos en el límite, a costa de hasta un fotograma más de retardo de entrada. Se "
                                 "aplica al instante.",
    [UI_MENU_REALTIME] = "La velocidad del juego sigue el reloj",
    [UI_MENU_REALTIME_DESC] = "Avanza el juego según el tiempo real, de modo que un fotograma lento no ralentiza el juego.",
    [UI_MENU_CLOTH_REALTIME] = "Telas según el reloj",
    [UI_MENU_CLOTH_REALTIME_DESC] = "Avanza también las telas y el pelo según el tiempo real.",
    [UI_MENU_HIGHFPS_FIX] = "Correcciones para fotogramas altos",
    [UI_MENU_HIGHFPS_FIX_DESC] = "Corrige la física que falla por encima de 30 fotogramas por segundo y los objetos que quedan transparentes o invisibles por encima de 60.",
    [UI_MENU_CLIMB_FIX] = "Corrección de la velocidad al trepar",
    [UI_MENU_CLIMB_FIX_DESC] = "Mantiene a velocidad completa la subida por paredes y escaleras por encima de 60 fotogramas por segundo, para que Link llegue arriba.",
    [UI_MENU_GAME_LANG] = "Idioma del juego", [UI_MENU_GAME_LANG_DESC] = "Idioma de los textos y las voces del juego.",
    [UI_MENU_GAME_LANG_AUTO] = "Automático",
    [UI_MENU_OPERATION_MODE] = "Modo de la consola",
    [UI_MENU_OPERATION_MODE_DESC] = "Si el juego cree que funciona en la base o en modo portátil.",
    [UI_MENU_MODE_DOCKED] = "En la base", [UI_MENU_MODE_HANDHELD] = "Portátil",
    [UI_MENU_ASSUME_FOCUS] = "Seguir en segundo plano",
    [UI_MENU_ASSUME_FOCUS_DESC] = "Sí (lo predeterminado): el juego sigue funcionando mientras estás en otra ventana, y el "
                                  "teclado queda en silencio hasta que la ventana vuelve a estar delante. No: se pausa cuando su "
                                  "ventana pierde el foco, como en la Switch al pulsar HOME; una carga interrumpida así puede "
                                  "detenerse para siempre.",

    [UI_MENU_PLAY_TIME] = "Tiempo de juego",
    [UI_MENU_PLAY_TIME_DESC] = "Tiempo en el juego sumando todas las partidas, contado mientras la ventana del juego tiene el foco "
                               "y el juego dibuja fotogramas. Se guarda en playtime.txt, en la carpeta del perfil, junto a las "
                               "partidas guardadas.",
    [UI_MENU_PLAY_TIME_HM] = "%llu h %02u min", [UI_MENU_PLAY_TIME_SESSION] = "esta partida: %s",
    [UI_MENU_MODS_BUILTIN] = "Integrados en el port", [UI_MENU_MODS_INSTALLED] = "Mods instalados",
    [UI_MENU_MODS_NONE] = "Aún no hay mods. Descomprime un mod en la carpeta de mods, una carpeta por mod con su carpeta romfs "
                          "dentro, y reinicia el juego. TKMM también puede exportar allí sus mods combinados.",
    [UI_MENU_MODS_OPEN] = "Abrir la carpeta de mods",
    [UI_MENU_MODS_OPEN_DESC] = "Muestra la carpeta de la que el juego carga los mods. Se crea si no existe.",
    [UI_MENU_MODS_ROW_DESC] = "Archivos del juego que sustituye: %u. Archivos nuevos que añade: %u.",
    [UI_MENU_MODS_OVERRIDDEN] = "%u de sus archivos proceden de %s, que está más arriba en la lista.",
    [UI_MENU_MODS_MOVE_HINT] = "El botón superior o Ctrl+Arriba lo sube, el botón izquierdo o Ctrl+Abajo lo baja. Cuando dos mods "
                               "cambian el mismo archivo, gana el que está más arriba. Los cambios se aplican tras reiniciar.",
    [UI_MENU_MODS_EMPTY] = "Esta carpeta no tiene archivos romfs, exefs ni cheats.",
    [UI_MENU_MODS_SUBSDK] = "Este mod trae su propio código de juego (un complemento subsdk o un main reemplazado). El port no "
                            "puede cargar módulos de código, así que todo el mod queda desactivado.",
    [UI_MENU_MODS_EXEFS_LATER] = "Este mod parchea el código del juego (exefs). El juego integra los parches en el próximo inicio "
                                 "(una reconstrucción de unos minutos); hasta entonces todo el mod queda desactivado.",
    [UI_MENU_MODS_CHEATS] = "Trucos: %d. Actívalos en la pestaña Trucos.",
    [UI_MENU_MODS_CHEATS_FILE] = "Los trucos de esta versión del juego se leen al iniciar de cheats\\%s en este mod.",
    [UI_MENU_MODS_BAD_NAME] = "El nombre de la carpeta contiene |, que la lista de mods usa entre nombres. Cambia el nombre de la carpeta.",
    [UI_MENU_MODS_OTHER_VERSION] = "%u de sus archivos están hechos para otra versión del juego y se omiten. Busca una versión de "
                                   "este mod hecha para tu versión del juego.",
    [UI_MENU_MODS_UNUSED] = "sin usar",
    [UI_MENU_MODS_FPS60] = "Parche de 60 FPS",
    [UI_MENU_MODS_FPS60_DESC] = "El parche propio del port para 60 fotogramas por segundo. Se configura con Límite de fotogramas "
                                "en la pestaña Juego.",
    [UI_MENU_MODS_RES] = "Parche de resolución",
    [UI_MENU_MODS_RES_DESC] = "El parche propio del port para resoluciones superiores a 1080p. Se configura con Resolución del "
                              "juego en la pestaña Pantalla.",
    [UI_MENU_MODS_QUICKMENU] = "Corrección del menú rápido",
    [UI_MENU_MODS_QUICKMENU_DESC] = "La corrección del port para el fondo negro de los menús de armas, escudos y materiales "
                                    "a 60 FPS.",

    [UI_MENU_VOLUME] = "Volumen", [UI_MENU_VOLUME_DESC] = "Volumen general del juego.",
    [UI_MENU_AUDIO_CHANNELS] = "Altavoces",
    [UI_MENU_AUDIO_CHANNELS_DESC] = "Salida estéreo o envolvente 5.1. Automático elige 5.1 cuando la configuración de altavoces de "
                                    "Windows tiene seis o más canales. Se aplica tras reiniciar.",
    [UI_MENU_CHANNELS_AUTO] = "Automático", [UI_MENU_CHANNELS_STEREO] = "Estéreo", [UI_MENU_CHANNELS_51] = "Envolvente 5.1",

    [UI_MENU_MOTION] = "Controles de movimiento", [UI_MENU_MOTION_DESC] = "Apuntar con el giroscopio del mando.",
    [UI_MENU_RUMBLE_STRENGTH] = "Intensidad de la vibración",
    [UI_MENU_RUMBLE_STRENGTH_DESC] = "Qué tan fuerte vibra el mando.",
    [UI_MENU_RUMBLE_CURVE] = "Curva de vibración",
    [UI_MENU_RUMBLE_CURVE_DESC] = "Los valores bajos refuerzan la vibración débil.",
    [UI_MENU_RUMBLE_TRIGGERS] = "Vibración de los gatillos (Xbox)",
    [UI_MENU_RUMBLE_TRIGGERS_DESC] = "Hace vibrar también los gatillos de un mando de Xbox.",
    [UI_MENU_PAD_LAYOUT] = "Disposición de los botones",
    [UI_MENU_PAD_LAYOUT_DESC] = "Cuál es el botón A. Nintendo: el de la derecha, como en una Switch. Xbox: el de abajo, como "
                                "indica un mando de Xbox (la cruz en un mando de PlayStation). Automático elige Nintendo para "
                                "los mandos con etiquetas de Nintendo y Xbox para los demás. El mando cuyos botones muestran las "
                                "indicaciones del juego se elige en Indicaciones de botones.",
    [UI_MENU_PAD_LAYOUT_AUTO] = "Automático", [UI_MENU_PAD_LAYOUT_NINTENDO] = "Nintendo (A a la derecha)",
    [UI_MENU_PAD_LAYOUT_XBOX] = "Xbox (A abajo)",
    [UI_MENU_PAD_LAYOUT_IN_USE] = "en uso: %s",
    [UI_MENU_PROMPTS] = "Indicaciones de botones",
    [UI_MENU_PROMPTS_DESC] = "Los botones de qué mando muestran las indicaciones del juego. Nintendo: las propias del juego. "
                             "Xbox: LB, RB, LT, RT, Menú, Vista y una cruceta, y A, B, X e Y según la etiqueta del botón que "
                             "pulsa la disposición de arriba (con Nintendo (A a la derecha) el botón de la derecha se llama B, "
                             "así que las indicaciones de A muestran B). PC: las teclas y los botones del ratón de la "
                             "disposición predeterminada del teclado (E, Mayús, Espacio, clic izquierdo...) y el ratón para la "
                             "cámara. El juego lee sus indicaciones al iniciar, así que cambian tras reiniciar. Forman parte de "
                             "los archivos del juego, así que un botón que reasignes en Controles conserva en pantalla su "
                             "indicación anterior.",
    [UI_MENU_PROMPTS_NINTENDO] = "Nintendo", [UI_MENU_PROMPTS_XBOX] = "Xbox", [UI_MENU_PROMPTS_PC] = "PC",
    [UI_MENU_PROMPTS_MISSING] = "faltan los archivos de indicaciones: ejecuta de nuevo la instalación",
    [UI_MENU_PROMPTS_FAILED] = "archivos de indicaciones dañados: ejecuta de nuevo la instalación",
    [UI_MENU_PROMPTS_RESTART] = "las indicaciones cambian tras reiniciar",
    [UI_MENU_PROMPTS_OFF_SHAPE] = "desactivadas con esta resolución del juego (no hay disposición hecha para ella)",
    [UI_MENU_PROMPTS_OFF_MODS] = "desactivadas: un mod de RomFS sustituye sus archivos",
    [UI_MENU_PROMPTS_OFF_PATCHES] = "desactivadas mientras TOTK_ROMFS_PATCHES esté definida",
    [UI_MENU_RESTART] = "Reiniciar ahora",
    [UI_MENU_RESTART_DESC] = "Cierra el juego y lo inicia de nuevo con las nuevas indicaciones. Se pierde el progreso desde la "
                             "última vez que guardaste.",
    [UI_MENU_RESTART_CONFIRM] = "¿Reiniciar el juego ahora? Se pierde el progreso desde la última vez que guardaste.",
    [UI_MENU_RESTART_YES] = "Reiniciar",
    [UI_MENU_RESTART_SAVING] = "El juego está guardando: podrás reiniciar en un momento.",
    [UI_MENU_GROUP_STICKS] = "Palancas analógicas",
    [UI_MENU_LSTICK_DEADZONE] = "Zona muerta de la palanca izquierda", [UI_MENU_RSTICK_DEADZONE] = "Zona muerta de la palanca derecha",
    [UI_MENU_STICK_DEADZONE_DESC] = "Cuánto se mueve la palanca de un mando antes de que el juego lo note. Súbela si el personaje o la cámara se mueven solos: suelta la palanca, lee \"palanca ahora\" y fija la zona muerta unos puntos porcentuales por encima.",
    [UI_MENU_STICK_DEADZONE_NOW] = "palanca ahora: %.0f %%",
    [UI_MENU_MOUSE_MODE] = "Cámara con el ratón",
    [UI_MENU_MOUSE_MODE_DESC] = "Directo: la cámara gira según cuánto se mueve el ratón, como en un juego de PC, sin zona muerta ni curva de palanca (juegos 1.4.3 y 1.2.1; el 1.1.0 usa emulación de palanca). Emulación de palanca: el ratón actúa como la palanca derecha.",
    [UI_MENU_MOUSE_STICK] = "Emulación de palanca",
    [UI_MENU_MOUSE_DIRECT] = "Directo",
    [UI_MENU_MENU_CURSOR] = "Ratón en los menús del juego",
    [UI_MENU_MENU_CURSOR_DESC] = "En los menús propios del juego (título, inventario, tiendas, opciones de diálogo) aparece el puntero: se selecciona el elemento que tiene debajo, el clic izquierdo confirma y el derecho retrocede. Solo cuando lo último que se usó fue el ratón o una tecla, así que un mando ve los menús sin cambios (juego 1.4.3).",
    [UI_MENU_MOUSE_SENS] = "Sensibilidad de la cámara con el ratón",
    [UI_MENU_MOUSE_SENS_DESC] = "Qué tan rápido gira la cámara con el ratón.",
    [UI_MENU_WALK_KEY] = "Tecla de caminar",
    [UI_MENU_WALK_KEY_DESC] = "Mantenla pulsada para inclinar las palancas del teclado solo a medias, de modo que el teclado "
                              "camine en vez de correr. La escala de caminar fija cuánto. Un mando y el ratón no se ven afectados.",
    [UI_MENU_WALK_KEY_TAKEN] = "también %s",
    [UI_MENU_WALK_SCALE] = "Escala de caminar",
    [UI_MENU_WALK_SCALE_DESC] = "La inclinación que reciben las palancas del teclado mientras se mantiene la tecla de caminar. La "
                                "mitad es caminar, la inclinación completa es correr.",
    [UI_MENU_STICK_SENS] = "Velocidad de la cámara con la palanca",
    [UI_MENU_STICK_SENS_DESC] = "Multiplica la velocidad propia del juego de la cámara con la palanca derecha, velocidad máxima "
                                "incluida, en cada una de las cinco sensibilidades de cámara del juego. El ratón conserva su "
                                "propio ajuste.",

    [UI_BINDS_HELP] = "Selecciona una celda y pulsa A o Intro, y después la tecla, el botón del ratón o el botón del mando. Supr o "
                      "X vacía una celda. Una tecla en uso se traslada aquí; un botón del mando se intercambia con el que "
                      "sustituye.",
    [UI_BINDS_COL_KEY1] = "Tecla 1", [UI_BINDS_COL_KEY2] = "Tecla 2", [UI_BINDS_COL_PAD] = "Mando",
    [UI_BINDS_GROUP_BUTTONS] = "Botones", [UI_BINDS_GROUP_MOVE] = "Movimiento", [UI_BINDS_GROUP_CAMERA] = "Cámara",
    [UI_BINDS_PRESS_KEY] = "Pulsa una tecla o un botón del ratón para %s",
    [UI_BINDS_PRESS_PAD] = "Pulsa un botón del mando para %s",
    [UI_BINDS_CANCEL_HINT] = "Esc cancela, o espera %d s",
    [UI_BINDS_TAKEN] = "%s se quitó de %s", [UI_BINDS_SWAPPED] = "%s se intercambió con %s",
    [UI_BINDS_RESET] = "Restablecer todos los botones",
    [UI_BINDS_RESET_DESC] = "Vuelve a las teclas y los botones del mando predeterminados. La disposición de botones de arriba se aplica de nuevo: los botones del mando reasignados se quedan donde los pusiste hasta este restablecimiento.",
    [UI_BIND_A] = "A", [UI_BIND_B] = "B", [UI_BIND_X] = "X", [UI_BIND_Y] = "Y",
    [UI_BIND_LCLICK] = "Clic de la palanca izquierda", [UI_BIND_RCLICK] = "Clic de la palanca derecha", [UI_BIND_L] = "L",
    [UI_BIND_R] = "R", [UI_BIND_ZL] = "ZL", [UI_BIND_ZR] = "ZR", [UI_BIND_PLUS] = "Más", [UI_BIND_MINUS] = "Menos",
    [UI_BIND_LEFT] = "Cruceta izquierda", [UI_BIND_UP] = "Cruceta arriba", [UI_BIND_RIGHT] = "Cruceta derecha",
    [UI_BIND_DOWN] = "Cruceta abajo",
    [UI_BIND_LSTICK_LEFT] = "Mover a la izquierda", [UI_BIND_LSTICK_UP] = "Mover adelante", [UI_BIND_LSTICK_RIGHT] = "Mover a la derecha",
    [UI_BIND_LSTICK_DOWN] = "Mover atrás",
    [UI_BIND_RSTICK_LEFT] = "Cámara a la izquierda", [UI_BIND_RSTICK_UP] = "Cámara arriba", [UI_BIND_RSTICK_RIGHT] = "Cámara a la derecha",
    [UI_BIND_RSTICK_DOWN] = "Cámara abajo",
    [UI_BIND_DESC_A] = "Interactuar, recoger, hablar.", [UI_BIND_DESC_B] = "Correr, cancelar, retroceder.",
    [UI_BIND_DESC_X] = "Saltar.", [UI_BIND_DESC_Y] = "Atacar.", [UI_BIND_DESC_LCLICK] = "Agacharse.",
    [UI_BIND_DESC_RCLICK] = "Vista en primera persona.", [UI_BIND_DESC_L] = "Habilidades.",
    [UI_BIND_DESC_R] = "Lanzar un arma.", [UI_BIND_DESC_ZL] = "Escudo, fijar objetivo.", [UI_BIND_DESC_ZR] = "Arco.",
    [UI_BIND_DESC_PLUS] = "Menú, inventario.", [UI_BIND_DESC_MINUS] = "Mapa.",
    [UI_BIND_DESC_DPAD] = "Selección rápida.",
    [UI_BIND_DESC_MOVE] = "Mueve a Link como la palanca izquierda. La palanca izquierda del mando también funciona siempre.",
    [UI_BIND_DESC_CAMERA] = "Gira la cámara como la palanca derecha. El ratón y la palanca derecha del mando también funcionan "
                            "siempre.",
    [UI_BIND_SHAKE] = "Agitar la palanca de la cámara",
    [UI_BIND_DESC_SHAKE] = "Agita la palanca derecha de lado a lado por el juego: separa una pieza fusionada de lo que sostiene "
                           "Ultramano. Mantener la tecla sigue agitando.",
    [UI_KEY_SPACE] = "Espacio", [UI_KEY_TAB] = "Tab", [UI_KEY_LSHIFT] = "Mayús izquierda", [UI_KEY_LCTRL] = "Ctrl izquierda",
    [UI_KEY_UP] = "Flecha arriba", [UI_KEY_DOWN] = "Flecha abajo", [UI_KEY_LEFT] = "Flecha izquierda",
    [UI_KEY_RIGHT] = "Flecha derecha",
    [UI_KEY_MOUSE_L] = "Ratón izquierdo", [UI_KEY_MOUSE_R] = "Ratón derecho", [UI_KEY_MOUSE_M] = "Ratón central",
    [UI_KEY_MOUSE_X1] = "Ratón 4", [UI_KEY_MOUSE_X2] = "Ratón 5",
    [UI_KEY_ENTER] = "Intro", [UI_KEY_BACKSPACE] = "Retroceso", [UI_KEY_RSHIFT] = "Mayús derecha",
    [UI_KEY_RCTRL] = "Ctrl derecha", [UI_KEY_LALT] = "Alt", [UI_KEY_CAPSLOCK] = "Bloq Mayús", [UI_KEY_INSERT] = "Insert",
    [UI_KEY_DELETE] = "Supr", [UI_KEY_HOME] = "Inicio", [UI_KEY_END] = "Fin", [UI_KEY_PAGEUP] = "Re Pág",
    [UI_KEY_PAGEDOWN] = "Av Pág", [UI_KEY_NUM_FMT] = "Num %s",
    [UI_PAD_SOUTH] = "Botón inferior", [UI_PAD_EAST] = "Botón derecho", [UI_PAD_WEST] = "Botón izquierdo",
    [UI_PAD_NORTH] = "Botón superior", [UI_PAD_LSTICK] = "Palanca izquierda", [UI_PAD_RSTICK] = "Palanca derecha",
    [UI_PAD_L] = "Botón superior izquierdo", [UI_PAD_R] = "Botón superior derecho", [UI_PAD_START] = "Inicio", [UI_PAD_BACK] = "Atrás",
    [UI_PAD_DPAD_LEFT] = "Cruceta izquierda", [UI_PAD_DPAD_UP] = "Cruceta arriba", [UI_PAD_DPAD_RIGHT] = "Cruceta derecha",
    [UI_PAD_DPAD_DOWN] = "Cruceta abajo", [UI_PAD_ZL] = "Gatillo izquierdo", [UI_PAD_ZR] = "Gatillo derecho",
    [UI_PAD_LPADDLE1] = "Botón trasero izquierdo", [UI_PAD_RPADDLE1] = "Botón trasero derecho",
    [UI_PAD_LPADDLE2] = "Botón trasero izquierdo 2", [UI_PAD_RPADDLE2] = "Botón trasero derecho 2",
    [UI_PAD_CROSS] = "Cruz", [UI_PAD_CIRCLE] = "Círculo", [UI_PAD_SQUARE] = "Cuadrado", [UI_PAD_TRIANGLE] = "Triángulo",

    [UI_MENU_AMIIBO_DIR] = "Carpeta de amiibo",
    [UI_MENU_AMIIBO_OPEN] = "Abrir",
    [UI_MENU_AMIIBO_DEFAULT_DIR] = "Carpeta predeterminada",
    [UI_MENU_AMIIBO_REFRESH] = "Actualizar",
    [UI_MENU_AMIIBO_PICK] = "Volcados de amiibo (archivos .bin o .nfc de 540 bytes):",
    [UI_MENU_AMIIBO_NO_DIR] = "No se puede abrir la carpeta %s",
    [UI_MENU_AMIIBO_READER] = "Lector NFC",
    [UI_MENU_AMIIBO_STATE] = "Lector:",
    [UI_MENU_AMIIBO_STATE_IDLE] = "listo",
    [UI_MENU_AMIIBO_STATE_SEARCHING] = "el juego espera un amiibo",
    [UI_MENU_AMIIBO_STATE_FOUND] = "amiibo en el lector",
    [UI_MENU_AMIIBO_STATE_MOUNTED] = "el juego está leyendo el amiibo",
    [UI_MENU_AMIIBO_STATE_OFF] = "apagado",
    [UI_MENU_AMIIBO_FILE] = "Archivo seleccionado:",
    [UI_MENU_AMIIBO_NO_FILE] = "(ningún archivo seleccionado)",
    [UI_MENU_AMIIBO_TAP] = "Acercar el amiibo",
    [UI_MENU_AMIIBO_REMOVE] = "Retirar el amiibo",
    [UI_MENU_AMIIBO_AUTO_REMOVE] = "Retirar el amiibo solo tras acercarlo",
    [UI_MENU_AMIIBO_HOLD] = "Tiempo en el lector",
    [UI_MENU_AMIIBO_SECONDS] = "%.1f s",
    [UI_MENU_AMIIBO_TAG] = "Amiibo en el lector:",
    [UI_MENU_AMIIBO_NO_TAG] = "No hay ningún amiibo en el lector.",
    [UI_MENU_AMIIBO_TAG_FILE] = "Archivo: %s",
    [UI_MENU_AMIIBO_TAG_ID] = "Personaje %s, serie %02x, número %04x, tipo %02x",
    [UI_MENU_AMIIBO_TAG_UID] = "UID: %s",
    [UI_MENU_AMIIBO_READY] = "Listo",
    [UI_MENU_AMIIBO_TAPPED] = "Acercado: %s",
    [UI_MENU_AMIIBO_LOAD_FAILED] = "No es un volcado de amiibo (NTAG215 de 540 bytes): %s",
    [UI_MENU_AMIIBO_REMOVED] = "Amiibo retirado",
    [UI_MENU_AMIIBO_STATUS] = "Estado: %s",
    [UI_MENU_MENU_LANG] = "Idioma del menú", [UI_MENU_MENU_LANG_DESC] = "Idioma de este menú y del contador.",
    [UI_MENU_MENU_LANG_AUTO] = "Automático (según Windows)",
    [UI_MENU_DISCORD_RPC] = "Discord Rich Presence",
    [UI_MENU_DISCORD_RPC_DESC] = "Muestra \"Playing TotK Recomp\" con el tiempo de juego en tu estado de Discord mientras el "
                                 "juego está en marcha. discord_app_id y discord_image en totk.ini eligen la aplicación.",

    [UI_MENU_ADV_WARNING] = "Estos ajustes sirven para pruebas y para resolver problemas. Cámbialos solo si sabes por qué.",
    [UI_MENU_GROUP_SCHED] = "Planificación", [UI_MENU_GROUP_SHADERS] = "Sombreadores y pipelines",
    [UI_MENU_GROUP_TEXMEM] = "Texturas y memoria", [UI_MENU_GROUP_FIXES] = "Correcciones de renderizado",
    [UI_MENU_GROUP_VULKAN] = "Vulkan", [UI_MENU_GROUP_DIAG] = "Diagnóstico",
    [UI_MENU_ASYNC_PREEMPT] = "Apropiación de núcleo virtual",
    [UI_MENU_ASYNC_PREEMPT_DESC] = "Permite que un hilo del juego interrumpa a otro en el mismo núcleo virtual.",
    [UI_MENU_PREEMPT_OFF] = "No", [UI_MENU_PREEMPT_ON] = "Sí", [UI_MENU_PREEMPT_INHERIT] = "Solo herencia",
    [UI_MENU_REPLAY_ASYNC] = "Preparación de fotogramas de GPU en su propio hilo",
    [UI_MENU_REPLAY_ASYNC_DESC] = "Prepara los fotogramas para la GPU en un hilo aparte en vez del hilo del juego. Añade hasta un fotograma de latencia.",
    [UI_MENU_REPLAY_WORKERS] = "Hilos de preparación de fotogramas de GPU",
    [UI_MENU_REPLAY_WORKERS_DESC] = "Graba los fotogramas grandes para la GPU en varios hilos a la vez. Ayuda cuando la CPU limita la tasa de fotogramas; requiere 8 o más procesadores lógicos. No graba en un solo hilo.",
    [UI_MENU_REPLAY_WORKERS_NOW] = "ahora: %s",
    [UI_MENU_TIMER_1MS] = "Temporizador de Windows de 1 ms",
    [UI_MENU_TIMER_1MS_DESC] = "Sube la resolución del temporizador del sistema a 1 ms para un ritmo de fotogramas más estable.",
    [UI_MENU_PIPE_WAIT] = "Gestión de dibujos en espera",
    [UI_MENU_PIPE_WAIT_DESC] = "Predeterminado mantiene la planificación actual con un trabajador reservado para los dibujos en espera; Antiguo (dibujo) usa la planificación anterior.",
    [UI_MENU_PIPE_WAIT_DEFAULT] = "Predeterminado", [UI_MENU_PIPE_WAIT_DRAW] = "Antiguo (dibujo)",
    [UI_MENU_PIPE_SKIP] = "Dibujos omitidos mientras se construye un pipeline",
    [UI_MENU_PIPE_SKIP_DESC] = "Qué dibujos se descartan en vez de esperar a su pipeline.",
    [UI_MENU_PIPE_SKIP_STRICT] = "Estricto", [UI_MENU_PIPE_SKIP_ANY] = "Cualquiera",
    [UI_MENU_PIPE_SKIP_FULLSCREEN] = "Pasadas a pantalla completa",
    [UI_MENU_LIB_PREBUILD] = "Preconstrucción de bibliotecas de etapas",
    [UI_MENU_LIB_PREBUILD_DESC] = "Construye de antemano, en segundo plano, las bibliotecas de etapas de los sombreadores.",
    [UI_MENU_LIB_PREBUILD_THREADS] = "Hilos de preconstrucción de bibliotecas de etapas",
    [UI_MENU_LIB_PREBUILD_THREADS_DESC] = "Hilos que se usan para la preconstrucción; Automático elige un número según tu CPU.",
    [UI_MENU_PIPE_PREBUILD] = "Preconstrucción de pipelines",
    [UI_MENU_PIPE_PREBUILD_DESC] = "Construye de antemano los pipelines conocidos.",
    [UI_MENU_PREWARM_XLATE] = "Precalentamiento del traductor",
    [UI_MENU_PREWARM_XLATE_DESC] = "Hilos que traducen los sombreadores conocidos al iniciar; Automático toma la mitad de los hilos de la CPU, como máximo 16.",
    [UI_MENU_GPL] = "Biblioteca de pipelines gráficos",
    [UI_MENU_GPL_DESC] = "Usa la extensión de biblioteca de pipelines de Vulkan; desactívala si tu controlador falla con ella.",
    [UI_MENU_ASTC] = "Decodificación de texturas ASTC", [UI_MENU_ASTC_DESC] = "Dónde se decodifican las texturas comprimidas ASTC.",
    [UI_MENU_ASTC_AUTO] = "Automática", [UI_MENU_ASTC_GPU] = "Sombreador de GPU", [UI_MENU_ASTC_NATIVE] = "Nativa",
    [UI_MENU_ASTC_CPU] = "CPU",
    [UI_MENU_MEMORY] = "Modo de memoria de GPU", [UI_MENU_MEMORY_DESC] = "Cómo se comparte la memoria del juego con la GPU.",
    [UI_MENU_MEMORY_AUTO] = "Automático", [UI_MENU_MEMORY_IMPORT] = "Importar", [UI_MENU_MEMORY_MIRROR] = "Espejo",
    [UI_MENU_MEMORY_VERIFY] = "Verificar",
    [UI_MENU_TEX_ALIAS] = "Alias de texturas", [UI_MENU_TEX_ALIAS_DESC] = "Una textura nueva creada sobre la memoria de una textura viva empieza con ese contenido en vez de vacía.",
    [UI_MENU_TEX_VRAM_PRESSURE] = "Límite de memoria de texturas", [UI_MENU_TEX_VRAM_PRESSURE_DESC] = "Porcentaje de la memoria de vídeo a partir del cual se descartan las texturas que el juego liberó (vuelven del almacenamiento cuando se usan de nuevo). 0 las conserva.",
    [UI_MENU_ASTC_BC] = "Comprimir ASTC a BC3", [UI_MENU_ASTC_BC_DESC] = "Con el decodificador de GPU: vuelve a codificar las texturas ASTC decodificadas a BC3, una cuarta parte de la memoria de vídeo. Las texturas se ven algo más pixeladas. Auto: activado para GPU con 4 GB de memoria de vídeo o menos.",
    [UI_MENU_LOD_GUARD] = "Protección de LOD", [UI_MENU_LOD_GUARD_DESC] = "Evita que durante un fotograma aparezcan objetos parpadeando en el lugar equivocado o un fondo lejano plano, esperando a que el juego termine de reescribir los datos de nivel de detalle antes de que la GPU los lea.",
    [UI_MENU_FIX_C0] = "Corrección del búfer de constantes",
    [UI_MENU_FIX_C0_DESC] = "Rellena el banco de constantes c0 reservado por el controlador (tamaños de almacenamiento, tamaño de despacho, tamaño de punto, instancia base) en vez de leer ceros.",
    [UI_MENU_FIX_ZERO] = "Poner a cero la memoria de GPU nueva",
    [UI_MENU_FIX_ZERO_DESC] = "Los grupos de memoria de GPU nuevos empiezan rellenos de ceros, como la memoria de la Switch.",
    [UI_MENU_BIND_SKIP] = "Omitir enlaces repetidos",
    [UI_MENU_BIND_SKIP_DESC] = "Optimización: omite los enlaces que no cambiaron.",
    [UI_MENU_SHADER_MEMO] = "Memoria de sombreadores", [UI_MENU_SHADER_MEMO_DESC] = "Optimización: recuerda las búsquedas de sombreadores.",
    [UI_MENU_PIPE_MEMO] = "Memoria de pipelines", [UI_MENU_PIPE_MEMO_DESC] = "Optimización: reutiliza el pipeline del dibujo anterior cuando su estado se repite.",
    [UI_MENU_TEX_MEMO] = "Memoria de enlaces de texturas", [UI_MENU_TEX_MEMO_DESC] = "Optimización: recuerda los descriptores de texturas entre dibujos.",
    [UI_MENU_DESC_TEMPLATE] = "Plantillas de descriptores",
    [UI_MENU_DESC_TEMPLATE_DESC] = "Optimización: actualiza los descriptores en bloque.",
    [UI_MENU_PUSH_SKIP] = "Omitir constantes push repetidas",
    [UI_MENU_PUSH_SKIP_DESC] = "Optimización: omite las constantes push que no cambiaron.",
    [UI_MENU_SPLIT_DRAWS] = "Envío anticipado a la GPU",
    [UI_MENU_SPLIT_DRAWS_DESC] = "Envía el trabajo de la GPU por partes cada N dibujos entre pasadas para que la GPU empiece antes; 0 lo desactiva.",
    [UI_MENU_VK_VALIDATION] = "Capa de validación",
    [UI_MENU_VK_VALIDATION_DESC] = "Comprueba cada llamada de Vulkan y ralentiza mucho el juego.",
    [UI_MENU_VK_VALIDATION_MISSING] = "La capa de validación no está instalada.",
    [UI_MENU_VK_DYN_VERTEX] = "Estado de vértices dinámico",
    [UI_MENU_VK_DYN_VERTEX_DESC] = "Fuerza la entrada de vértices dinámica activada o desactivada; lo predeterminado depende de la GPU.",
    [UI_MENU_VK_DYN_A2C] = "Alfa a cobertura dinámico",
    [UI_MENU_VK_DYN_A2C_DESC] = "Fuerza el alfa a cobertura dinámico activado o desactivado. Predeterminado: desactivado con el "
                                "controlador de AMD para RDNA 2 y anteriores, que con él dibuja los personajes en negro, y con "
                                "el controlador de Intel para Windows en GPU integradas, que dibuja ruido de colores en el "
                                "follaje; activado en el resto.",
    [UI_MENU_VK_SUBGROUP_PIN] = "Fijar el tamaño de subgrupo", [UI_MENU_VK_SUBGROUP_PIN_DESC] = "Fija el tamaño de subgrupo de los sombreadores.",
    [UI_MENU_TRACE] = "Registro de rendimiento", [UI_MENU_TRACE_DESC] = "Escribe los tiempos de fotograma en un archivo.",
    [UI_MENU_FLIGHT] = "Registrador de vuelo",
    [UI_MENU_FLIGHT_DESC] = "Guarda un breve historial y lo graba cuando un fotograma se congela.",
    [UI_MENU_FLIGHT_SPIKE_MS] = "Umbral de congelación (ms)",
    [UI_MENU_FLIGHT_SPIKE_MS_DESC] = "Tiempo de fotograma que cuenta como congelación.",
    [UI_MENU_PASS_CRUMBS] = "Marcas de pasadas de GPU",
    [UI_MENU_PASS_CRUMBS_DESC] = "Tras un fallo de la GPU, el registro nombra la pasada de renderizado que estaba en curso. Automático: solo AMD. Sí: también NVIDIA, con un pequeño coste.",
    [UI_MENU_INPUT_RECORD] = "Grabación de entradas",
    [UI_MENU_INPUT_RECORD_DESC] = "Escribe tus entradas en un archivo para los informes de errores.",
    [UI_MENU_OPEN_LOG] = "Abrir la carpeta de registros", [UI_MENU_OPEN_LOG_DESC] = "Muestra la carpeta con los archivos de registro.",
    [UI_MENU_COPY_INFO] = "Copiar la información del sistema",
    [UI_MENU_COPY_INFO_DESC] = "Copia al portapapeles la GPU, el controlador, la CPU, la RAM y los ajustes cambiados.",
    [UI_MENU_COPY_INFO_DONE] = "Copiado", [UI_MENU_ACTION_FAILED] = "no funcionó",
    [UI_MENU_RESET] = "Restablecer todos los ajustes", [UI_MENU_RESET_DESC] = "Se aplica a todas las pestañas.",
    [UI_MENU_RESET_CONFIRM] = "¿Restablecer todos los ajustes a sus valores predeterminados?", [UI_MENU_RESET_YES] = "Restablecer",
    [UI_MENU_RESET_NO] = "Cancelar",

    [UI_MENU_OV_FPS] = "%.0f FPS", [UI_MENU_OV_FRAME_MS] = "%.1f ms", [UI_MENU_OV_GAME_PCT] = "juego %.0f%%",
    [UI_MENU_OV_MAX_MS] = "máx. %.1f ms",
    [UI_MENU_OV_FPS_LABEL] = "FPS", [UI_MENU_OV_FRAME_LABEL] = "Fotograma", [UI_MENU_OV_FMT_FPS] = "%.0f",
    [UI_MENU_OV_LOW1] = "1%% bajo %.0f", [UI_MENU_OV_LOW01] = "0.1%% bajo %.0f",
    [UI_MENU_OV_GPU] = "GPU", [UI_MENU_OV_CPU] = "CPU", [UI_MENU_OV_RAM] = "RAM", [UI_MENU_OV_VRAM] = "VRAM",
    [UI_MENU_OV_TEMP] = "Temperatura", [UI_MENU_OV_POWER] = "Potencia", [UI_MENU_OV_CLOCK] = "Reloj",
    [UI_MENU_OV_NONE] = "-",
    [UI_MENU_OV_REPLAY] = "Preparación de GPU", [UI_MENU_OV_GPU_TIME] = "GPU ocupada por fotograma",
    [UI_MENU_OV_PIPE_WAIT] = "Esperas de pipelines",
    [UI_MENU_OV_SHADER_WAIT] = "Esperas de sombreadores", [UI_MENU_OV_TEX_UPLOADS] = "Subidas de texturas",
    [UI_MENU_OV_FMT_PCT] = "%.0f%%", [UI_MENU_OV_FMT_TEMP] = "%.0f °C", [UI_MENU_OV_FMT_POWER] = "%.0f W",
    [UI_MENU_OV_FMT_MHZ] = "%.0f MHz", [UI_MENU_OV_FMT_GB] = "%.1f GB", [UI_MENU_OV_FMT_GB_OF] = "/ %.1f GB",
    [UI_MENU_OV_GAME_GB] = "juego %.1f GB", [UI_MENU_OV_FMT_MS] = "%.2f ms", [UI_MENU_OV_PER_FRAME] = "%.1f por fotograma",
    [UI_MENU_OV_WINDOW] = "Ventana", [UI_MENU_OV_OUTPUT] = "Imagen del juego", [UI_MENU_OV_SCENE] = "Escena 3D",
    [UI_MENU_OV_SCALING] = "Escalado", [UI_MENU_OV_LIMIT] = "Límite de fotogramas", [UI_MENU_OV_PRESENT] = "Presentación",
    [UI_MENU_OV_FMT_SIZE] = "%ux%u", [UI_MENU_OV_FMT_SCENE] = "%ux%u, %.0f%%",
    [UI_MENU_OV_FMT_FSR] = "AMD FSR 1, nitidez %d%%", [UI_MENU_OV_SCALE_NONE] = "ninguno (1:1)",
    [UI_MENU_OV_SCALE_BILINEAR] = "bilineal",
    [UI_MENU_OV_SCALE_DOWN] = "bilineal (FSR solo amplía)",
    [UI_MENU_TAB_CHEATS] = "Trucos",
    [UI_MENU_CHEATS_NONE] = "No hay trucos para esta versión del juego. Los trucos de un mod son un archivo de trucos de "
                            "Atmosphere en su carpeta cheats, con el nombre de la compilación del juego: "
                            "mods\\<mod>\\cheats\\%s. Reinicia el juego para leerlos. Para los trucos que cambian el código "
                            "del juego, el juego se reconstruye al iniciar (unos minutos).",
    [UI_MENU_CHEATS_MOD] = "%s: %d trucos",
    [UI_MENU_CHEAT_CODE_DESC] = "Cambia el código del juego. Se activa al instante.",
    [UI_MENU_CHEAT_KEY_DESC] = "Cambia el código del juego mientras mantienes %s. Se activa al instante.",
    [UI_MENU_CHEAT_DATA_DESC] = "Escribe en la memoria del juego en cada fotograma mientras está activado.",
    [UI_MENU_CHEAT_MASTER] = "siempre activado",
    [UI_MENU_CHEAT_MASTER_DESC] = "El código maestro del archivo: siempre está activado.",
    [UI_MENU_CHEAT_RESTORE] = "no hace falta",
    [UI_MENU_CHEAT_RESTORE_DESC] = "Solo vuelve a escribir el código propio del juego. Desactivar un truco ya lo hace.",
    [UI_MENU_CHEAT_REBUILD] = "tras la reconstrucción",
    [UI_MENU_CHEAT_REBUILD_DESC] = "Este truco cambia el código del juego y es nuevo o se editó después de construir el juego. "
                                   "El juego lo integra la próxima vez que se inicie (unos minutos), o usa Reconstruir el "
                                   "juego ahora.",
    [UI_MENU_CHEAT_UNSUPPORTED] = "no compatible",
    [UI_MENU_CHEAT_UNSUPPORTED_DESC] = "Este truco no se puede usar con esta versión del juego:",
    [UI_MENU_CHEATS_REBUILD] = "Reconstruir el juego ahora",
    [UI_MENU_CHEATS_REBUILD_DESC] = "Algunos trucos cambian el código del juego y no están integrados en él: son nuevos o se "
                                    "editaron, o su última reconstrucción falló o se canceló. Cierra el juego; la instalación "
                                    "los integra (unos minutos) y vuelve a iniciar el juego. Se pierde el progreso desde la "
                                    "última vez que guardaste.",
    [UI_MENU_MODS_EXEFS_UNSUPPORTED] = "Este mod parchea el código del juego (exefs) de una forma que el port no puede aplicar, "
                                       "así que todo el mod queda desactivado. %s",
    [UI_MENU_MODS_EXEFS_OTHER] = "Sus parches de código (exefs) están hechos para otra versión del juego.",
    [UI_MENU_MODS_EXEFS_ON] = "Parches de código en uso: %d archivos.",
    [UI_MENU_MODS_REPLACES_BUILTIN] = "Parches de código en uso: %d archivos. Sustituyen al parche propio del port %s, que "
                                      "sigue desactivado mientras este mod esté activado.",
