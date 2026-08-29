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
 * 7. Allocate memory for kernel
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

    // Step 6: Allocate memory for kernel
    // Allocate pool memory to hold the kernel image
    UINT64 kernel_size = file_info->FileSize;
    void *kernel_buffer = NULL;
    status = SystemTable->BootServices->AllocatePool(
        EfiLoaderData,
        kernel_size,
        &kernel_buffer);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot allocate memory for kernel\r\n");
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Step 6: Memory allocated\r\n");

    // Step 7: Read kernel into memory
    // Load kernel binary from disk into allocated memory
    UINTN read_size = kernel_size;
    status = kernel_file->Read(kernel_file, &read_size, kernel_buffer);
    ASSERT_EFI(SystemTable->ConOut, status, L"[!] FATAL: Cannot read kernel file\r\n");
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] Step 7: Kernel read into memory\r\n");

    // Step 8: Cleanup resources
    // Close file handles and free temporary memory
    kernel_file->Close(kernel_file);
    root_dir->Close(root_dir);
    SystemTable->BootServices->FreePool(file_info);

    // Display success message
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[+] SUCCESS: Kernel loaded and ready!\r\n");
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"[!] Halting...\r\n");

    // Halt CPU - in production, this would jump to kernel entry point
    while (1)
    {
        __asm__("hlt");
    }

    return EFI_SUCCESS;
}