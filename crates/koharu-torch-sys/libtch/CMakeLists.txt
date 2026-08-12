cmake_minimum_required(VERSION 3.5 FATAL_ERROR)
project(tch)

find_package(Torch REQUIRED)

add_library(koharu-torch SHARED torch_api.cpp torch_api_generated.cpp)
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    # The selected Torch backend is downloaded and loaded globally before this
    # shim is opened. Keep the build-time CPU libraries out of DT_NEEDED so the
    # AppImage bundler does not capture them as application-owned dependencies.
    target_include_directories(koharu-torch PRIVATE "${TORCH_INCLUDE_DIRS}")
    separate_arguments(TORCH_COMPILE_OPTIONS NATIVE_COMMAND "${TORCH_CXX_FLAGS}")
    target_compile_options(koharu-torch PRIVATE ${TORCH_COMPILE_OPTIONS})
else()
    target_link_libraries(koharu-torch "${TORCH_LIBRARIES}")
endif()
set_property(TARGET koharu-torch PROPERTY CXX_STANDARD 17)
set_property(TARGET koharu-torch PROPERTY WINDOWS_EXPORT_ALL_SYMBOLS ON)
install(TARGETS koharu-torch DESTINATION .)
