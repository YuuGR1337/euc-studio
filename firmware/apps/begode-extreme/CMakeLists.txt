# Begode Extreme main-board image: build/<preset>/euc-fw-begode-extreme.{elf,bin,map}
euc_add_image(euc-fw-begode-extreme
    BOARD begode-extreme
    SOURCES
        main.c
        euc_info.c)
