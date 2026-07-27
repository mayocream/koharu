cmake_minimum_required(VERSION 3.24)

project(koharu-llama-cpp LANGUAGES C CXX)

set(LLAMA_CPP_SOURCE_DIR "" CACHE PATH "Path to a llama.cpp checkout")
set(KOHARU_LLAMA_BACKEND "" CACHE STRING "Backend to build: CUDA, VULKAN, or METAL")
set_property(CACHE KOHARU_LLAMA_BACKEND PROPERTY STRINGS CUDA VULKAN METAL)

if(NOT IS_DIRECTORY "${LLAMA_CPP_SOURCE_DIR}")
    message(FATAL_ERROR "LLAMA_CPP_SOURCE_DIR is not a directory: ${LLAMA_CPP_SOURCE_DIR}")
endif()

string(TOUPPER "${KOHARU_LLAMA_BACKEND}" KOHARU_LLAMA_BACKEND)
if(NOT KOHARU_LLAMA_BACKEND MATCHES "^(CUDA|VULKAN|METAL)$")
    message(FATAL_ERROR "KOHARU_LLAMA_BACKEND must be CUDA, VULKAN, or METAL")
endif()

file(REAL_PATH "${LLAMA_CPP_SOURCE_DIR}" LLAMA_CPP_SOURCE_DIR)

# llama.cpp uses BUILD_SHARED_LIBS for both libllama and every ggml library.
# Start with shared libraries here, then override it only in ggml's nested
# project scope with ggml-static.cmake.
set(BUILD_SHARED_LIBS ON CACHE BOOL "" FORCE)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_PROJECT_INCLUDE_BEFORE "${CMAKE_CURRENT_LIST_DIR}/ggml-static.cmake")

# Build only the public runtime.  These cache values intentionally replace
# llama.cpp's standalone defaults and keep CI focused on the linkage graph.
set(LLAMA_BUILD_COMMON OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_APP OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_MTMD OFF CACHE BOOL "" FORCE)
set(LLAMA_OPENSSL OFF CACHE BOOL "" FORCE)
set(GGML_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(GGML_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(GGML_BACKEND_DL OFF CACHE BOOL "" FORCE)
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
set(GGML_LLAMAFILE OFF CACHE BOOL "" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "" FORCE)

# Make each job opt in to exactly one accelerator backend.  The CPU backend
# remains enabled because llama.cpp requires it for operations not offloaded.
set(GGML_CUDA OFF CACHE BOOL "" FORCE)
set(GGML_HIP OFF CACHE BOOL "" FORCE)
set(GGML_VULKAN OFF CACHE BOOL "" FORCE)
set(GGML_METAL OFF CACHE BOOL "" FORCE)
set(GGML_BLAS OFF CACHE BOOL "" FORCE)
set(GGML_ACCELERATE OFF CACHE BOOL "" FORCE)

if(KOHARU_LLAMA_BACKEND STREQUAL "CUDA")
    set(GGML_CUDA ON CACHE BOOL "" FORCE)
elseif(KOHARU_LLAMA_BACKEND STREQUAL "VULKAN")
    set(GGML_VULKAN ON CACHE BOOL "" FORCE)
elseif(KOHARU_LLAMA_BACKEND STREQUAL "METAL")
    set(GGML_METAL ON CACHE BOOL "" FORCE)
    set(GGML_METAL_EMBED_LIBRARY ON CACHE BOOL "" FORCE)
    set(GGML_BLAS ON CACHE BOOL "" FORCE)
    set(GGML_ACCELERATE ON CACHE BOOL "" FORCE)
endif()

add_subdirectory("${LLAMA_CPP_SOURCE_DIR}" llama.cpp)

get_target_property(_koharu_llama_type llama TYPE)
if(NOT _koharu_llama_type STREQUAL "SHARED_LIBRARY")
    message(FATAL_ERROR "Expected llama to be shared, got ${_koharu_llama_type}")
endif()

# Collect every static ggml target created by the selected backend.  Force the
# archives into libllama so public ggml/gguf entry points and backend registries
# are retained even when libllama itself does not reference a particular symbol.
# Upstream target graph:
# https://github.com/ggml-org/llama.cpp/blob/3d1c3a8975f970a8e5f99ea648733087b52124c5/ggml/src/CMakeLists.txt#L242-L288
function(koharu_collect_targets directory output)
    get_property(_targets DIRECTORY "${directory}" PROPERTY BUILDSYSTEM_TARGETS)
    get_property(_subdirectories DIRECTORY "${directory}" PROPERTY SUBDIRECTORIES)
    foreach(_subdirectory IN LISTS _subdirectories)
        koharu_collect_targets("${_subdirectory}" _subdirectory_targets)
        list(APPEND _targets ${_subdirectory_targets})
    endforeach()
    set(${output} "${_targets}" PARENT_SCOPE)
endfunction()

koharu_collect_targets("${LLAMA_CPP_SOURCE_DIR}" _koharu_upstream_targets)
set(_koharu_ggml_targets)
foreach(_target IN LISTS _koharu_upstream_targets)
    if(_target MATCHES "^ggml($|-)")
        get_target_property(_target_type "${_target}" TYPE)
        if(NOT _target_type STREQUAL "STATIC_LIBRARY")
            message(FATAL_ERROR "Expected ${_target} to be static, got ${_target_type}")
        endif()
        list(APPEND _koharu_ggml_targets "${_target}")
    endif()
endforeach()

if(NOT _koharu_ggml_targets)
    message(FATAL_ERROR "No static ggml targets were found")
endif()

string(JOIN "," _koharu_ggml_targets_csv ${_koharu_ggml_targets})
set_property(
    TARGET llama
    PROPERTY LINK_LIBRARY_OVERRIDE "WHOLE_ARCHIVE,${_koharu_ggml_targets_csv}"
)

if(WIN32)
    # The static ggml objects become part of llama.dll.  Mark their public C
    # APIs for export without propagating dllimport definitions to libllama.
    foreach(_target IN LISTS _koharu_ggml_targets)
        target_compile_definitions(
            "${_target}"
            PRIVATE
                GGML_BUILD
                GGML_SHARED
                GGML_BACKEND_BUILD
                GGML_BACKEND_SHARED
        )
    endforeach()
    set_target_properties(llama PROPERTIES WINDOWS_EXPORT_ALL_SYMBOLS ON)
endif()

# Produce one unversioned library with the name consumed by Koharu.  Avoid
# libllama.so.* and libllama.*.dylib symlink sets in the artifact directory.
set_target_properties(
    llama
    PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
        LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
        ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/archive"
)
set_property(TARGET llama PROPERTY VERSION)
set_property(TARGET llama PROPERTY SOVERSION)

foreach(_config DEBUG RELEASE RELWITHDEBINFO MINSIZEREL)
    set_target_properties(
        llama
        PROPERTIES
            RUNTIME_OUTPUT_DIRECTORY_${_config} "${CMAKE_BINARY_DIR}/bin"
            LIBRARY_OUTPUT_DIRECTORY_${_config} "${CMAKE_BINARY_DIR}/bin"
            ARCHIVE_OUTPUT_DIRECTORY_${_config} "${CMAKE_BINARY_DIR}/archive"
    )
endforeach()

message(STATUS "Koharu llama.cpp backend: ${KOHARU_LLAMA_BACKEND}")
message(STATUS "Static ggml targets: ${_koharu_ggml_targets}")
