cmake_minimum_required(VERSION 3.5 FATAL_ERROR)
project(tch)

find_package(Torch REQUIRED)

add_library(koharu-torch SHARED torch_api.cpp torch_api_generated.cpp)
target_link_libraries(koharu-torch "${TORCH_LIBRARIES}")
set_property(TARGET koharu-torch PROPERTY CXX_STANDARD 17)
set_property(TARGET koharu-torch PROPERTY WINDOWS_EXPORT_ALL_SYMBOLS ON)
install(TARGETS koharu-torch DESTINATION .)
