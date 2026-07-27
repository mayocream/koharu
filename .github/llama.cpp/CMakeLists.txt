cmake_minimum_required(VERSION 3.24)

project(llama_cpp LANGUAGES C CXX)

set(LLAMA_CPP_SOURCE_DIR "" CACHE PATH "Path to a llama.cpp checkout")
set(LLAMA_BACKEND "" CACHE STRING "Backend to build: CUDA, HIP, VULKAN, or METAL")
set_property(CACHE LLAMA_BACKEND PROPERTY STRINGS CUDA HIP VULKAN METAL)

if(NOT IS_DIRECTORY "${LLAMA_CPP_SOURCE_DIR}")
    message(FATAL_ERROR "LLAMA_CPP_SOURCE_DIR is not a directory: ${LLAMA_CPP_SOURCE_DIR}")
endif()

string(TOUPPER "${LLAMA_BACKEND}" LLAMA_BACKEND)
if(NOT LLAMA_BACKEND MATCHES "^(CUDA|HIP|VULKAN|METAL)$")
    message(FATAL_ERROR "LLAMA_BACKEND must be CUDA, HIP, VULKAN, or METAL")
endif()

file(REAL_PATH "${LLAMA_CPP_SOURCE_DIR}" LLAMA_CPP_SOURCE_DIR)

# Keep libllama and libmtmd shared while the nested GGML project builds static
# libraries that are folded into libllama.
set(BUILD_SHARED_LIBS ON CACHE BOOL "" FORCE)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
set(CMAKE_PROJECT_INCLUDE_BEFORE "${CMAKE_CURRENT_LIST_DIR}/ggml-static.cmake")

set(LLAMA_BUILD_COMMON OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_TOOLS OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_SERVER OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_APP OFF CACHE BOOL "" FORCE)
set(LLAMA_BUILD_MTMD ON CACHE BOOL "" FORCE)
set(LLAMA_OPENSSL OFF CACHE BOOL "" FORCE)
set(GGML_BUILD_TESTS OFF CACHE BOOL "" FORCE)
set(GGML_BUILD_EXAMPLES OFF CACHE BOOL "" FORCE)
set(GGML_BACKEND_DL OFF CACHE BOOL "" FORCE)
set(GGML_NATIVE OFF CACHE BOOL "" FORCE)
set(GGML_LLAMAFILE OFF CACHE BOOL "" FORCE)
set(GGML_OPENMP OFF CACHE BOOL "" FORCE)

foreach(_backend CUDA HIP VULKAN METAL BLAS ACCELERATE)
    set(GGML_${_backend} OFF CACHE BOOL "" FORCE)
endforeach()
set(GGML_${LLAMA_BACKEND} ON CACHE BOOL "" FORCE)

string(TOLOWER "${LLAMA_BACKEND}" _backend)
set(_ggml_targets ggml-base ggml ggml-cpu "ggml-${_backend}")
if(LLAMA_BACKEND STREQUAL "METAL")
    set(GGML_METAL_EMBED_LIBRARY ON CACHE BOOL "" FORCE)
    set(GGML_BLAS ON CACHE BOOL "" FORCE)
    set(GGML_ACCELERATE ON CACHE BOOL "" FORCE)
    list(APPEND _ggml_targets ggml-blas)
endif()

add_subdirectory("${LLAMA_CPP_SOURCE_DIR}" llama.cpp)

foreach(_target llama mtmd)
    get_target_property(_type "${_target}" TYPE)
    if(NOT _type STREQUAL "SHARED_LIBRARY")
        message(FATAL_ERROR "Expected ${_target} to be shared, got ${_type}")
    endif()
endforeach()

foreach(_target IN LISTS _ggml_targets)
    if(NOT TARGET "${_target}")
        message(FATAL_ERROR "Missing GGML target: ${_target}")
    endif()
    get_target_property(_type "${_target}" TYPE)
    if(NOT _type STREQUAL "STATIC_LIBRARY")
        message(FATAL_ERROR "Expected ${_target} to be static, got ${_type}")
    endif()
endforeach()

# Retain every GGML object in libllama, including backend registries that
# libllama does not reference directly.
string(JOIN "," _ggml_targets_csv ${_ggml_targets})
set_property(
    TARGET llama
    PROPERTY LINK_LIBRARY_OVERRIDE "WHOLE_ARCHIVE,${_ggml_targets_csv}"
)

# libmtmd uses the GGML symbols exported by libllama instead of embedding a
# second copy of the static GGML graph.
set_property(TARGET llama PROPERTY INTERFACE_LINK_LIBRARIES "")
set_property(TARGET mtmd PROPERTY LINK_LIBRARIES "llama;Threads::Threads")
set_property(TARGET mtmd PROPERTY INTERFACE_LINK_LIBRARIES "llama")
target_include_directories(mtmd PRIVATE "${LLAMA_CPP_SOURCE_DIR}/ggml/include")

if(WIN32)
    foreach(_target IN LISTS _ggml_targets)
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

set_target_properties(
    llama mtmd
    PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
        LIBRARY_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/bin"
        ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}/archive"
)
set_property(TARGET llama mtmd PROPERTY VERSION)
set_property(TARGET llama mtmd PROPERTY SOVERSION)

foreach(_config DEBUG RELEASE RELWITHDEBINFO MINSIZEREL)
    set_target_properties(
        llama mtmd
        PROPERTIES
            RUNTIME_OUTPUT_DIRECTORY_${_config} "${CMAKE_BINARY_DIR}/bin"
            LIBRARY_OUTPUT_DIRECTORY_${_config} "${CMAKE_BINARY_DIR}/bin"
            ARCHIVE_OUTPUT_DIRECTORY_${_config} "${CMAKE_BINARY_DIR}/archive"
    )
endforeach()

message(STATUS "llama.cpp backend: ${LLAMA_BACKEND}")
message(STATUS "Static GGML targets: ${_ggml_targets}")
