cmake_minimum_required(VERSION 3.5 FATAL_ERROR)
project(tch)

find_package(Torch REQUIRED)

add_library(koharu_torch_shim SHARED torch_api.cpp torch_api_generated.cpp)
target_link_libraries(koharu_torch_shim "${TORCH_LIBRARIES}")
set_property(TARGET koharu_torch_shim PROPERTY CXX_STANDARD 17)
install(TARGETS koharu_torch_shim DESTINATION .)
