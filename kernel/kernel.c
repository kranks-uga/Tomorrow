static void serial_put(char c)
{
    __asm__ volatile("outb %0, %1" : : "a"(c), "Nd"((unsigned short)0x3F8));
}

static void serial_str(const char *s)
{
    while (*s)
    {
        if (*s == '\n')
            serial_put('\r');
        serial_put(*s++);
    }
}

void kernel_main(unsigned long long fb_base, unsigned int width, unsigned int height, unsigned int ppsl)
{
    serial_str("[KERNEL] I am alive!\n");
    volatile unsigned int *fb = (volatile unsigned int *)fb_base;

    // Каждый пиксель = 4 байта. Шаг строки — ppsl (pixels per scanline),
    // а НЕ width: у режима может быть паддинг в конце строки.
    for (unsigned int y = 0; y < height; y++)
    {
        for (unsigned int x = 0; x < width; x++)
        {
            fb[y * ppsl + x] = 0x00FF0000;
        }
    }

    while (1)
    {
        __asm__ volatile("hlt");
    }
}