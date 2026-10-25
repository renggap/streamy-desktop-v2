cmake_minimum_required(VERSION 3.15)
project(libmp3lame C)

set(CMAKE_C_STANDARD 99)

include_directories(
    vendor/lame/include
    vendor/lame/libmp3lame
)

add_definitions(-DHAVE_CONFIG_H)

set(LAME_SOURCES
    vendor/lame/libmp3lame/bitstream.c
    vendor/lame/libmp3lame/encoder.c
    vendor/lame/libmp3lame/fft.c
    vendor/lame/libmp3lame/gain_analysis.c
    vendor/lame/libmp3lame/id3tag.c
    vendor/lame/libmp3lame/lame.c
    vendor/lame/libmp3lame/newmdct.c
    vendor/lame/libmp3lame/presets.c
    vendor/lame/libmp3lame/psymodel.c
    vendor/lame/libmp3lame/quantize.c
    vendor/lame/libmp3lame/quantize_pvt.c
    vendor/lame/libmp3lame/reservoir.c
    vendor/lame/libmp3lame/set_get.c
    vendor/lame/libmp3lame/tables.c
    vendor/lame/libmp3lame/takehiro.c
    vendor/lame/libmp3lame/util.c
    vendor/lame/libmp3lame/vbrquantize.c
    vendor/lame/libmp3lame/VbrTag.c
    vendor/lame/libmp3lame/version.c
)

if(WIN32)
    add_library(mp3lame SHARED ${LAME_SOURCES} lame.def)
    set_target_properties(mp3lame PROPERTIES
        OUTPUT_NAME "libmp3lame"
        ARCHIVE_OUTPUT_NAME "mp3lame"
    )
else()
    add_library(mp3lame SHARED ${LAME_SOURCES})
    set_target_properties(mp3lame PROPERTIES
        OUTPUT_NAME "mp3lame"
    )
    target_link_libraries(mp3lame m)
endif()
