void kernel_main()
{
    volatile unsigned char *vga = (volatile unsigned char *)0xB8000;
    vga[0] = 'H';
    vga[1] = 0x0F;
    vga[2] = 'E';
    vga[3] = 0x0F;
    vga[4] = 'L';
    vga[5] = 0x0F;
    vga[6] = 'L';
    vga[7] = 0x0F;
    vga[8] = '0';
    vga[9] = 0x0F;

    while (1)
    {
    }
}