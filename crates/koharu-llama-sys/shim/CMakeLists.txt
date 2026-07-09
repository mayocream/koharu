cmake_minimum_required(VERSION 3.15 FATAL_ERROR)
project(koharu_llama_shim LANGUAGES CXX)

add_library(
    koharu_llama_shim SHARED
    "${KOHARU_LLAMA_COMMON_SHIM_SOURCE}"
    "${KOHARU_LLAMA_JSON_SCHEMA_SOURCE}"
    "${KOHARU_LLAMA_COMMON_SUPPORT_SOURCE}"
)
target_include_directories(
    koharu_llama_shim PRIVATE
    "${KOHARU_LLAMA_ROOT_DIR}"
    "${KOHARU_LLAMA_INCLUDE_DIR}"
    "${KOHARU_LLAMA_COMMON_DIR}"
    "${KOHARU_LLAMA_VENDOR_DIR}"
)
target_compile_features(koharu_llama_shim PRIVATE cxx_std_17)

if(WIN32)
    target_compile_definitions(koharu_llama_shim PRIVATE WIN32_LEAN_AND_MEAN NOMINMAX)
endif()

install(TARGETS koharu_llama_shim DESTINATION .)
