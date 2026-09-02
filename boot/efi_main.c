/**
 * @file efi_main.c
 * @brief Secure UEFI Bootloader
 *
 * A minimal UEFI bootloader that loads kernel from disk.
 * Designed with security in mind - no external dependencies.
 *
 * Architecture: x86_64
 * Target: Custom secure operating system
 */

#include "efi.h"

#define EFI_PAGE_SIZE 0x1000ULL

/**
 * @brief Print an unsigned 32-bit value to the UEFI console in decimal.
 *
 * There is no printf in a freestanding bootloader; this is just enough to
 * log the graphics modes the firmware offers.
 */
static void print_u32(EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL *out, UINT32 v)
{
    CHAR16 buf[11];
    int i = 10;
    buf[i--] = 0;
    if (v == 0)
        buf[i--] = L'0';
    while (v > 0 && i >= 0)
    {
        buf[i--] = (CHAR16)(L'0' + (v % 10));
        v /= 10;
    }
    out->OutputString(out, &buf[i + 1]);
}

/**
 * @brief Hard resolution request. If both are non-zero the loader picks
 * exactly this mode when the firmware offers it (before trying the list
 * below). Set to 0 to just use the priority list.
 */
#define REQUEST_WIDTH 0
#define REQUEST_HEIGHT 0

/**
 * @brief Preferred display resolutions, tried first and in this exact order.
 *
 * If none of these are available the loader falls back to a scored pick
 * (16:9 > 16:10 > 4:3 > other, then largest area).
 */
static const UINT32 PREFERRED_MODES[][2] = {
    {1920, 1080},
    {1600, 900},
    {1366, 768},
    {1280, 720},
    {1280, 1024},
    {1440, 900},
    {1680, 1050},
    {1280, 800},
    {1024, 768},
    {800, 600},
    {640, 480},
};
#define PREFERRED_MODE_COUNT (sizeof(PREFERRED_MODES) / sizeof(PREFERRED_MODES[0]))

/**
 * @brief UEFI Entry Point
 *
 * This function is called by UEFI firmware when the bootloader is loaded.
 * It performs the following steps:
 * 1. Initialize console and display welcome message
 * 2. Load Loaded Image Protocol
 * 3. Load File System Protocol
 * 4. Open root directory
 * 5. Open kernel file
 * 6. Get kernel file information
 * 7. Allocate memory for kernel (with headroom for .bss/stack)
 * 8. Read kernel into memory
 * 9. Cleanup and halt
 *
 * @param ImageHandle Handle to the loaded image
 * @param SystemTable Pointer to UEFI System Table
 * @return EFI_STATUS Status code
 */
EFI_STATUS __attribute__((ms_abi)) efi_main(EFI_HANDLE ImageHandle, EFI_SYSTEM_TABLE *SystemTable)
{
    // Step 0: Clear screen and display welcome message
    SystemTable->ConOut->ClearScreen(SystemTable->ConOut);
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[*] Secure Bootloader starting...\r\n");

    // Step 1: Load Loaded Image Protocol
    // This protocol provides information about the loaded image
    EFI_LOADED_IMAGE_PROTOCOL *loaded_image = NULL;
    EFI_GUID load_image_guid = EFI_LOADED_IMAGE_PROTOCOL_GUID;
    EFI_STATUS status = SystemTable->BootServices->HandleProtocol(
        ImageHandle,
        &load_image_guid,
        (void **)&loaded_image);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot get Loaded Image Protocol\r\n");
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Step 1: Loaded Image OK\r\n");

    // Step 2: Load File System Protocol
    // This allows us to access the file system
    EFI_GUID efi_fs_guid = EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID;
    EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *file_system = NULL;
    status = SystemTable->BootServices->HandleProtocol(
        loaded_image->DeviceHandle,
        &efi_fs_guid,
        (void **)&file_system);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot get File System Protocol\r\n");
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Step 2: File System OK\r\n");

    // Step 3: Open root directory
    // Access the root directory of the boot device
    EFI_FILE_PROTOCOL *root_dir = NULL;
    status = file_system->OpenVolume(file_system, &root_dir);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot open root directory\r\n");
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Step 3: Root Dir OK\r\n");

    // Step 4: Open kernel file
    // Open kernel.bin for reading
    EFI_FILE_PROTOCOL *kernel_file = NULL;
    status = root_dir->Open(
        root_dir,
        &kernel_file,
        L"kernel.bin",
        EFI_FILE_MODE_READ,
        0);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot open kernel.bin\r\n");
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Step 4: Kernel file opened\r\n");

    // Step 5: Get file information
    // Retrieve kernel file size and metadata
    EFI_FILE_INFO *file_info = NULL;
    UINTN info_size = sizeof(EFI_FILE_INFO) + 256;
    status = SystemTable->BootServices->AllocatePool(
        EfiLoaderData,
        info_size,
        (void **)&file_info);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot allocate memory for file info\r\n");

    EFI_GUID file_info_guid = EFI_FILE_INFO_ID;
    status = kernel_file->GetInfo(
        kernel_file,
        &file_info_guid,
        &info_size,
        file_info);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot get file info\r\n");
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Step 5: File info retrieved\r\n");

    // Step 6: Allocate memory for kernel at ANY address the firmware picks.
    // This only works because kernel.c is now compiled WITHOUT -fno-pie
    // (see build.sh) — gcc emits RIP-relative addressing for globals/string
    // literals, so the code is truly position independent and needs no
    // fixed load address and no runtime relocations. kernel.asm already
    // uses `lea rsp, [rel stack_top]` for the same reason.
    //
    // We over-allocate: the flat binary carries only the file bytes, but the
    // kernel's .bss (BSS + stack, see kernel.asm) lives past end-of-file at an
    // offset determined by the linker's page-aligned section layout. Reserve
    // 1 MiB of headroom so `lea rsp, [rel stack_top]` lands inside our buffer.
    UINT64 kernel_size = file_info->FileSize;
    EFI_PHYSICAL_ADDRESS kernel_addr = 0;
    UINTN kernel_bytes = kernel_size + (1ULL << 20);
    UINTN kernel_pages = (kernel_bytes + EFI_PAGE_SIZE - 1) / EFI_PAGE_SIZE;
    // EfiLoaderCode, а не EfiLoaderData: на прошивках со строгой защитой памяти
    // (DxeNxMemoryProtectionPolicy) страницы *Data мапятся NX, и `call` в ядро
    // ловит #PF -> тройная ошибка. LoaderCode остаётся RWX (исполнение + запись
    // стека/.bss в той же области).
    status = SystemTable->BootServices->AllocatePages(
        AllocateAnyPages,
        EfiLoaderCode,
        kernel_pages,
        &kernel_addr);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot allocate memory for kernel\r\n");
    void *kernel_buffer = (void *)kernel_addr;
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Step 6: Memory allocated\r\n");

    // Step 7: Read kernel into memory
    // Load kernel binary from disk into allocated memory
    UINTN read_size = kernel_size;
    status = kernel_file->Read(kernel_file, &read_size, kernel_buffer);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot read kernel file\r\n");

    // Zero everything past the file image: this is the kernel's .bss/stack
    // region, which objcopy drops from the flat binary (NOBITS section).
    for (UINTN i = read_size; i < kernel_pages * EFI_PAGE_SIZE; i++)
        ((UINT8 *)kernel_buffer)[i] = 0;
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Step 7: Kernel read into memory\r\n");

    // Step 8: Cleanup resources
    // Close file handles and free temporary memory
    kernel_file->Close(kernel_file);
    root_dir->Close(root_dir);
    SystemTable->BootServices->FreePool(file_info);

    // Display success message
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] SUCCESS: Kernel loaded and ready!\r\n");

    EFI_GUID gop_guid = EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID;
    EFI_GRAPHICS_OUTPUT_PROTOCOL *gop = NULL;
    status = SystemTable->BootServices->LocateProtocol(&gop_guid, NULL, (void **)&gop);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot get GOP\r\n");

    // Enumerate every mode the firmware offers, log it, and pick one:
    //   1. first exact hit from PREFERRED_MODES (in priority order), else
    //   2. best scored mode: aspect class (16:9 > 16:10 > 4:3 > other),
    //      then largest pixel area.
    // If nothing usable is found, keep the mode that is already active.
    UINT32 best_mode = gop->Mode->Mode;
    int best_pref = -1;      // index into PREFERRED_MODES, -1 = no preferred hit
    UINT64 best_score = 0;   // used only while best_pref < 0

    EFI_GRAPHICS_OUTPUT_MODE_INFORMATION *info = NULL;
    UINTN size_of_info = 0;
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[*] GOP modes:\r\n");
    for (UINT32 i = 0; i < gop->Mode->MaxMode; i++)
    {
        if (gop->QueryMode(gop, i, &size_of_info, &info) != EFI_SUCCESS)
            continue;

        UINT32 w = info->HorizontalResolution;
        UINT32 h = info->VerticalResolution;
        EFI_GRAPHICS_PIXEL_FORMAT fmt = info->PixelFormat;
        SystemTable->BootServices->FreePool(info);
        info = NULL;

        // The kernel writes 32-bit pixels straight into the linear framebuffer,
        // so only direct 8-8-8-8 RGB/BGR modes are usable. PixelBitMask would
        // need per-channel shifting; PixelBltOnly has no linear framebuffer.
        if (fmt != PixelRedGreenBlueReserved8BitPerColor &&
            fmt != PixelBlueGreenRedReserved8BitPerColor)
            continue;

        SystemTable->ConOut->OutputString(SystemTable->ConOut, L"    ");
        print_u32(SystemTable->ConOut, w);
        SystemTable->ConOut->OutputString(SystemTable->ConOut, L"x");
        print_u32(SystemTable->ConOut, h);
        SystemTable->ConOut->OutputString(SystemTable->ConOut, L"\r\n");

        // An explicit REQUEST_WIDTH/HEIGHT wins over everything else.
        if (REQUEST_WIDTH != 0 && REQUEST_HEIGHT != 0 &&
            w == (UINT32)REQUEST_WIDTH && h == (UINT32)REQUEST_HEIGHT)
        {
            best_pref = -2; // sentinel: locked, stop considering other modes
            best_mode = i;
            continue;
        }
        if (best_pref == -2)
            continue;

        int pref = -1;
        for (UINTN p = 0; p < PREFERRED_MODE_COUNT; p++)
            if (PREFERRED_MODES[p][0] == w && PREFERRED_MODES[p][1] == h)
            {
                pref = (int)p;
                break;
            }

        if (pref >= 0)
        {
            if (best_pref < 0 || pref < best_pref)
            {
                best_pref = pref;
                best_mode = i;
            }
            continue;
        }

        if (best_pref < 0) // no preferred match yet: fall back to scoring
        {
            UINT64 aspect;
            if (w * 9 == h * 16)
                aspect = 3;
            else if (w * 10 == h * 16)
                aspect = 2;
            else if (w * 3 == h * 4)
                aspect = 1;
            else
                aspect = 0;
            UINT64 score = (aspect << 40) | ((UINT64)w * (UINT64)h);
            if (score > best_score)
            {
                best_score = score;
                best_mode = i;
            }
        }
    }

    status = gop->SetMode(gop, best_mode);
    if (status != EFI_SUCCESS)
        SystemTable->ConOut->OutputString(
            SystemTable->ConOut, L"[!] WARN: SetMode failed, keeping current mode\r\n");
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Selected mode: ");
    print_u32(SystemTable->ConOut, gop->Mode->Info->HorizontalResolution);
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"x");
    print_u32(SystemTable->ConOut, gop->Mode->Info->VerticalResolution);
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"\r\n");

    // Превращаем адрес kernel_buffer в указатель на функцию
    typedef void (*kernel_entry_t)(UINT64 fb_base, UINT32 width, UINT32 height, UINT32 pixels_per_scanline);
    kernel_entry_t kernel_entry = (kernel_entry_t)kernel_buffer;

    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[!] Jumping to kernel...\r\n");

    kernel_entry(
        gop->Mode->FrameBufferBase,
        gop->Mode->Info->HorizontalResolution,
        gop->Mode->Info->VerticalResolution,
        gop->Mode->Info->PixelsPerScanLine);

    // Halt CPU - in production, this would jump to kernel entry point
    while (1)
    {
        __asm__("hlt");
    }

    return EFI_SUCCESS;
}