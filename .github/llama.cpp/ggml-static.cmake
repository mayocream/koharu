# This hook runs in each nested project() scope.  Keep llama.cpp's outer
# library shared while making the ggml subproject and all of its backends
# static, so the final runtime does not require separate ggml libraries.
# Upstream option scopes:
# https://github.com/ggml-org/llama.cpp/blob/3d1c3a8975f970a8e5f99ea648733087b52124c5/CMakeLists.txt#L62
# https://github.com/ggml-org/llama.cpp/blob/3d1c3a8975f970a8e5f99ea648733087b52124c5/ggml/CMakeLists.txt#L85-L86
file(REAL_PATH "${CMAKE_CURRENT_SOURCE_DIR}" _current_source)
file(REAL_PATH "${LLAMA_CPP_SOURCE_DIR}/ggml" _ggml_source)

if(_current_source STREQUAL _ggml_source)
    set(BUILD_SHARED_LIBS OFF)
endif()
