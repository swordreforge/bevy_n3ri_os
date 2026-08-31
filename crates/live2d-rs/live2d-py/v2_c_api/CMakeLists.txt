# ──────── V2 C API wrapper (for Rust FFI) ────────
set(V2_C_API_TARGET v2_c_api)

add_library(${V2_C_API_TARGET} STATIC
    v2_c_api.cpp
)

target_include_directories(${V2_C_API_TARGET}
    PUBLIC  ${CMAKE_CURRENT_SOURCE_DIR}
    PRIVATE ${CMAKE_SOURCE_DIR}/Live2D/Glad/include
    PRIVATE ${CMAKE_SOURCE_DIR}/Live2D/Common
)

target_link_libraries(${V2_C_API_TARGET}
    PUBLIC V2
    PRIVATE glad
)

if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    target_link_libraries(${V2_C_API_TARGET} PRIVATE stdc++fs)
endif()

target_compile_features(${V2_C_API_TARGET} PUBLIC cxx_std_17)
set_target_properties(${V2_C_API_TARGET} PROPERTIES POSITION_INDEPENDENT_CODE ON)

# Alias for external consumption
add_library(Live2D::V2_C_API ALIAS ${V2_C_API_TARGET})
