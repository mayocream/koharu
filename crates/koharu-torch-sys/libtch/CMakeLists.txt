cmake_minimum_required(VERSION 3.15 FATAL_ERROR)
project(tch LANGUAGES CXX)

find_package(Torch REQUIRED)

add_library(koharu-torch SHARED torch_api.cpp torch_api_generated.cpp)
target_include_directories(koharu-torch PRIVATE ${TORCH_INCLUDE_DIRS})
target_link_libraries(koharu-torch PRIVATE ${TORCH_LIBRARIES})
target_compile_features(koharu-torch PRIVATE cxx_std_20)

if(TORCH_CXX_FLAGS)
  separate_arguments(TORCH_CXX_FLAGS_LIST NATIVE_COMMAND "${TORCH_CXX_FLAGS}")
  target_compile_options(koharu-torch PRIVATE ${TORCH_CXX_FLAGS_LIST})
endif()

if(MSVC)
  # PyTorch's static TorchConfig omits the CUDA libraries of torch_cuda.
  find_package(CUDAToolkit REQUIRED)
  find_library(KOHARU_CUDNN_LIBRARY cudnn
    HINTS "$ENV{CUDNN_ROOT}"
    PATH_SUFFIXES lib lib/x64
    REQUIRED
  )
  target_link_libraries(
    koharu-torch
    PRIVATE
      CUDA::cuda_driver
      CUDA::cudart
      CUDA::cublas
      CUDA::cublasLt
      CUDA::cufft
      CUDA::curand
      CUDA::cusolver
      CUDA::cusparse
      CUDA::nvrtc
      "${KOHARU_CUDNN_LIBRARY}"
  )
  target_link_options(koharu-torch PRIVATE "/DELAYLOAD:nvcuda.dll")
  target_link_libraries(koharu-torch PRIVATE delayimp)
  set_property(
    TARGET koharu-torch
    PROPERTY MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>"
  )
elseif(APPLE)
  # PyTorch's static TorchConfig omits the frameworks used by its MPS objects.
  target_link_options(
    koharu-torch
    PRIVATE
      "LINKER:-weak_framework,Foundation"
      "LINKER:-weak_framework,IOKit"
      "LINKER:-weak_framework,Metal"
      "LINKER:-weak_framework,MetalPerformanceShaders"
      "LINKER:-weak_framework,MetalPerformanceShadersGraph"
  )
endif()

set_property(TARGET koharu-torch PROPERTY WINDOWS_EXPORT_ALL_SYMBOLS ON)
install(TARGETS koharu-torch DESTINATION .)
