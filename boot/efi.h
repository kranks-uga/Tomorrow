#ifndef EFI_H
#define EFI_H

// ==== Базовые типы ====
typedef unsigned char UINT8;
typedef unsigned short UINT16;
typedef unsigned int UINT32;
typedef unsigned long long UINT64;
typedef long long INT64;
typedef unsigned long long UINTN; // размер = размеру указателя (мы целимся только в x86-64)
typedef UINT16 CHAR16;            // UEFI строки — UTF-16 (L"...")
typedef UINT64 EFI_STATUS;
typedef void *EFI_HANDLE;

#define EFI_SUCCESS 0

// ==== Общий заголовок таблиц UEFI ====
typedef struct
{
    UINT64 Signature;
    UINT32 Revision;
    UINT32 HeaderSize;
    UINT32 CRC32;
    UINT32 Reserved;
} EFI_TABLE_HEADER;

// ==== Протокол вывода текста ====
typedef struct EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL
{
    void *Reset; // заглушка, не используем сейчас
    EFI_STATUS(__attribute__((ms_abi)) * OutputString)(
        struct EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL *This,
        CHAR16 *String);
    void *TestString;   // заглушка
    void *QueryMode;    // заглушка
    void *SetMode;      // заглушка
    void *SetAttribute; // заглушка
    EFI_STATUS(__attribute__((ms_abi)) * ClearScreen)(
        struct EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL *This);
    // остальные поля протокола пока не нужны — SetCursorPosition, EnableCursor, Mode
} EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL;

// ==== Boot Services — пока заглушка целиком, распишем позже, когда понадобится ====
typedef struct
{
    EFI_TABLE_HEADER Hdr;
    // ... заполним, когда будем читать файлы/получать карту памяти
} EFI_BOOT_SERVICES;

// ==== Главная системная таблица ====
typedef struct
{
    EFI_TABLE_HEADER Hdr;
    CHAR16 *FirmwareVendor;
    UINT32 FirmwareRevision;
    EFI_HANDLE ConsoleInHandle;
    void *ConIn; // заглушка, не используем
    EFI_HANDLE ConsoleOutHandle;
    EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL *ConOut;
    EFI_HANDLE StandardErrorHandle;
    void *StdErr;          // заглушка
    void *RuntimeServices; // заглушка (пока не нужно)
    EFI_BOOT_SERVICES *BootServices;
    UINTN NumberOfTableEntries;
    void *ConfigurationTable;
} EFI_SYSTEM_TABLE;

#endif