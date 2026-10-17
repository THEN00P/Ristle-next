/* The bundled FDK source logs SBR overflows through Android's internal liblog
   (platform builds only). Stub it for NDK builds. */
#pragma once
#define android_errorWriteLog(tag, subtag) ((void)0)
