#ifndef EFI_H
#define EFI_H

#define IN
#define OUT
#define OPTIONAL

// ==== Базовые типы ====
typedef unsigned char UINT8;
typedef unsigned short UINT16;
typedef unsigned int UINT32;
typedef unsigned long long UINT64;
typedef long long INT64;
typedef unsigned long long UINTN;
typedef UINT16 CHAR16;
typedef UINT64 EFI_STATUS;
typedef void *EFI_HANDLE;
typedef UINT64 EFI_PHYSICAL_ADDRESS;
typedef UINT64 EFI_VIRTUAL_ADDRESS;

typedef EFI_STATUS(__attribute__((ms_abi)) * EFI_IMAGE_UNLOAD)(EFI_HANDLE ImageHandle);

// ==== Коды ответов ====
#define EFI_SUCCESS 0
#define EFI_BUFFER_TOO_SMALL 0x8000000000000005
#define EFI_NOT_FOUND 0x800000000000000E

// ==== Константы режимов файлов ====
#define EFI_FILE_MODE_READ 0x0000000000000001
#define EFI_FILE_MODE_WRITE 0x0000000000000002
#define EFI_FILE_MODE_CREATE 0x8000000000000000

// ==== GUID-ы основных протоколов ====
typedef struct
{
    UINT32 Data1;
    UINT16 Data2;
    UINT16 Data3;
    UINT8 Data4[8];
} EFI_GUID;

#define EFI_LOADED_IMAGE_PROTOCOL_GUID \
    {0x5B1B31A1, 0x9562, 0x11d2, {0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B}}

#define EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID \
    {0x964E5B22, 0x6459, 0x11D2, {0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B}}

#define EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID \
    {0x9042A9DE, 0x23DC, 0x4A38, {0x96, 0xFB, 0x7A, 0xDE, 0xD0, 0x80, 0x51, 0x6A}}

#define EFI_FILE_INFO_ID \
    {0x09576E92, 0x6D3F, 0x11D2, {0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B}}

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
    void *Reset;
    EFI_STATUS(__attribute__((ms_abi)) * OutputString)(
        struct EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL *This,
        CHAR16 *String);
    void *TestString;
    void *QueryMode;
    void *SetMode;
    void *SetAttribute;
    EFI_STATUS(__attribute__((ms_abi)) * ClearScreen)(
        struct EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL *This);
} EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL;

// ==== Пути устройств ====
typedef struct
{
    UINT8 Type;
    UINT8 SubType;
    UINT16 Length;
} EFI_DEVICE_PATH_PROTOCOL;

// ==== Типы памяти ====
typedef enum
{
    EfiReservedMemoryType,
    EfiLoaderCode,
    EfiLoaderData,
    EfiBootServicesCode,
    EfiBootServicesData,
    EfiRuntimeServicesCode,
    EfiRuntimeServicesData,
    EfiConventionalMemory,
    EfiUnusableMemory,
    EfiACPIReclaimMemory,
    EfiACPIMemoryNVS,
    EfiMemoryMappedIO,
    EfiMemoryMappedIOPortSpace,
    EfiPalCode,
    EfiMaxMemoryType
} EFI_MEMORY_TYPE;

typedef enum
{
    AllocateAnyPages,
    AllocateMaxAddress,
    AllocateAddress,
    MaxAllocateType
} EFI_ALLOCATE_TYPE;

typedef struct
{
    UINT32 Type;
    EFI_PHYSICAL_ADDRESS PhysicalStart;
    EFI_VIRTUAL_ADDRESS VirtualStart;
    UINT64 NumberOfPages;
    UINT64 Attribute;
} EFI_MEMORY_DESCRIPTOR;

// ==== Протокол работы с графикой (GOP) ====
typedef enum
{
    PixelRedGreenBlueReserved8BitPerColor,
    PixelBlueGreenRedReserved8BitPerColor,
    PixelBitMask,
    PixelBltOnly,
    PixelFormatMax
} EFI_GRAPHICS_PIXEL_FORMAT;

typedef struct
{
    UINT32 RedMask;
    UINT32 GreenMask;
    UINT32 BlueMask;
    UINT32 ReservedMask;
} EFI_PIXEL_BITMASK;

typedef struct
{
    UINT32 Version;
    UINT32 HorizontalResolution;
    UINT32 VerticalResolution;
    EFI_GRAPHICS_PIXEL_FORMAT PixelFormat;
    EFI_PIXEL_BITMASK PixelInformation;
    UINT32 PixelsPerScanLine;
} EFI_GRAPHICS_OUTPUT_MODE_INFORMATION;

typedef struct
{
    UINT32 MaxMode;
    UINT32 Mode;
    EFI_GRAPHICS_OUTPUT_MODE_INFORMATION *Info;
    UINTN SizeOfInfo;
    EFI_PHYSICAL_ADDRESS FrameBufferBase; // <- Физический адрес видеопамяти
    UINTN FrameBufferSize;                // <- Размер кадрового буфера в байтах
} EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE;

typedef struct EFI_GRAPHICS_OUTPUT_PROTOCOL
{
    void *QueryMode;
    EFI_STATUS(__attribute__((ms_abi)) * SetMode)(
        struct EFI_GRAPHICS_OUTPUT_PROTOCOL *This,
        UINT32 ModeNumber);
    void *Blt;
    EFI_GRAPHICS_OUTPUT_PROTOCOL_MODE *Mode;
} EFI_GRAPHICS_OUTPUT_PROTOCOL;

// ==== Протокол работы с файлами (Файловая система) ====
typedef struct _EFI_FILE_PROTOCOL EFI_FILE_PROTOCOL;

typedef struct _EFI_FILE_PROTOCOL
{
    UINT64 Revision;
    EFI_STATUS(__attribute__((ms_abi)) * Open)(
        EFI_FILE_PROTOCOL *This,
        EFI_FILE_PROTOCOL **NewHandle,
        CHAR16 *FileName,
        UINT64 OpenMode,
        UINT64 Attributes);
    EFI_STATUS(__attribute__((ms_abi)) * Close)(
        EFI_FILE_PROTOCOL *This);
    void *Delete;
    EFI_STATUS(__attribute__((ms_abi)) * Read)(
        EFI_FILE_PROTOCOL *This,
        UINTN *BufferSize,
        void *Buffer);
    void *Write;
    void *GetPosition;
    void *SetPosition;
    EFI_STATUS(__attribute__((ms_abi)) * GetInfo)(
        EFI_FILE_PROTOCOL *This,
        EFI_GUID *InformationType,
        UINTN *BufferSize,
        void *Buffer);
    void *SetInfo;
    void *Flush;
} EFI_FILE_PROTOCOL;

typedef struct
{
    UINT64 Size;
    UINT64 FileSize; // <- Размер файла на диске
    UINT64 PhysicalSize;
    void *CreateTime; // Упрощено
    void *LastAccessTime;
    void *ModificationTime;
    UINT64 Attribute;
    CHAR16 FileName[1]; // Динамическое имя файла в UTF-16
} EFI_FILE_INFO;

typedef struct EFI_SIMPLE_FILE_SYSTEM_PROTOCOL
{
    UINT64 Revision;
    EFI_STATUS(__attribute__((ms_abi)) * OpenVolume)(
        struct EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *This,
        EFI_FILE_PROTOCOL **Root);
} EFI_SIMPLE_FILE_SYSTEM_PROTOCOL;

// ==== Протокол загруженного образа ====
typedef struct
{
    UINT32 Revision;
    EFI_HANDLE ParentHandle;
    struct EFI_SYSTEM_TABLE *SystemTable;

    EFI_HANDLE DeviceHandle;
    EFI_DEVICE_PATH_PROTOCOL *FilePath;
    void *Reserved;

    UINT32 LoadOptionsSize;
    void *LoadOptions;

    void *ImageBase;
    UINT64 ImageSize;
    EFI_MEMORY_TYPE ImageCodeType;
    EFI_MEMORY_TYPE ImageDataType;
    EFI_IMAGE_UNLOAD Unload;
} EFI_LOADED_IMAGE_PROTOCOL;

// ==== Boot Services (Все функции на своих местах согласно спецификации) ====
typedef struct EFI_BOOT_SERVICES
{
    EFI_TABLE_HEADER Hdr;

    // Сервисы управления задачами (TPL)
    void *RaiseTPL;
    void *RestoreTPL;

    // Сервисы управления памятью
    EFI_STATUS(__attribute__((ms_abi)) * AllocatePages)(
        EFI_ALLOCATE_TYPE Type,
        EFI_MEMORY_TYPE MemoryType,
        UINTN Pages,
        EFI_PHYSICAL_ADDRESS *Memory);
    EFI_STATUS(__attribute__((ms_abi)) * FreePages)(
        EFI_PHYSICAL_ADDRESS Memory,
        UINTN Pages);
    EFI_STATUS(__attribute__((ms_abi)) * GetMemoryMap)(
        UINTN *MemoryMapSize,
        EFI_MEMORY_DESCRIPTOR *MemoryMap,
        UINTN *MapKey,
        UINTN *DescriptorSize,
        UINT32 *DescriptorVersion);
    EFI_STATUS(__attribute__((ms_abi)) * AllocatePool)(
        EFI_MEMORY_TYPE MemoryType,
        UINTN Size,
        void **Buffer);
    EFI_STATUS(__attribute__((ms_abi)) * FreePool)(
        void *Buffer);

    // Сервисы событий и таймеров
    void *CreateEvent;
    void *SetTimer;
    void *WaitForEvent;
    void *SignalEvent;
    void *CloseEvent;
    void *CheckEvent;

    // Сервисы поддержки протоколов
    void *InstallProtocolInterface;
    void *ReinstallProtocolInterface;
    void *UninstallProtocolInterface;
    EFI_STATUS(__attribute__((ms_abi)) * HandleProtocol)(
        EFI_HANDLE Handle,
        EFI_GUID *Protocol,
        void **Interface);
    void *Reserved;
    void *RegisterProtocolNotify;

    // Сервисы поиска хэндлов
    void *LocateHandle;
    void *LocateDevicePath;
    void *InstallConfigurationTable;

    // Сервисы загрузки образов
    void *ImageLoad;
    void *ImageStart;
    void *Exit;
    void *ImageUnload;
    EFI_STATUS(__attribute__((ms_abi)) * ExitBootServices)(
        EFI_HANDLE ImageHandle,
        UINTN MapKey);

    // Другие сервисы
    void *GetNextMonotonicCount;
    void *Stall;
    void *SetWatchdogTimer;

    // Драйверные сервисы
    void *ConnectController;
    void *DisconnectController;

    // Сервисы открытия/закрытия протоколов напрямую
    void *OpenProtocol;
    void *CloseProtocol;
    void *OpenProtocolInformation;

    // Библиотечные сервисы
    void *ProtocolsPerHandle;
    void *LocateHandleBuffer;
    EFI_STATUS(__attribute__((ms_abi)) * LocateProtocol)(
        EFI_GUID *Protocol,
        void *Registration,
        void **Interface);
} EFI_BOOT_SERVICES;

// ==== Таблица конфигурации (для поиска ACPI) ====
typedef struct
{
    EFI_GUID VendorGuid;
    void *VendorTable;
} EFI_CONFIGURATION_TABLE;

// ==== Главная системная таблица ====
typedef struct EFI_SYSTEM_TABLE
{
    EFI_TABLE_HEADER Hdr;
    CHAR16 *FirmwareVendor;
    UINT32 FirmwareRevision;
    EFI_HANDLE ConsoleInHandle;
    void *ConIn;
    EFI_HANDLE ConsoleOutHandle;
    EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL *ConOut;
    EFI_HANDLE StandardErrorHandle;
    void *StdErr;
    void *RuntimeServices;
    EFI_HANDLE ConsoleErrorHandle;
    EFI_BOOT_SERVICES *BootServices;
    UINTN NumberOfTableEntries;
    EFI_CONFIGURATION_TABLE *ConfigurationTable; // <- Позволит найти указатель на ACPI RSDP
} EFI_SYSTEM_TABLE;

#endif