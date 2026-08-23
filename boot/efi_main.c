#include "efi.h"

EFI_STATUS __attribute__((ms_abi)) efi_main(EFI_HANDLE ImageHandle, EFI_SYSTEM_TABLE *SystemTable)
{
    SystemTable->ConOut->OutputString(SystemTable->ConOut, L"Hello from UEFI!\n");
    while (1)
    {
    }
    return EFI_SUCCESS;
}