# Injected with CMAKE_PROJECT_INCLUDE when whisper.cpp is built for Windows or
# Linux (see .github/workflows/check.yml, job rust-native). GGML_NATIVE=ON, the
# default, compiles for the build machine's CPU (-march=native), so a binary
# built on a CI runner dies with an illegal instruction on an older CPU.
# Adapted from github.com/Zackriya-Solutions/meetily/.github/force-portable-ggml.cmake @ a2cb62e827da7ef59f65064c97233efb2313878e (MIT)
# whisper-rs-sys forwards CMAKE_* variables, so inject this after project().
# Avoid specializing native code for the CI runner. Which ISA extensions are
# then compiled in is left to ggml's own GGML_* defaults (none are set here).
set(GGML_NATIVE OFF CACHE BOOL "Build without host CPU specialization" FORCE)
