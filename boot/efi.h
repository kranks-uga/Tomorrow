#ifndef EFI_H
#define EFI_H

// ============================================================================
// UEFI Header Definitions
// Minimal UEFI headers for x86_64 bootloader development
// ============================================================================

// Standard UEFI calling convention macros
#define IN
#define OUT
#define OPTIONAL

// Null pointer definition
#define NULL ((void *)0)

// ============================================================================
// Basic Type Definitions
// ============================================================================
typedef unsigned char UINT8;
typedef unsigned short UINT16;
typedef unsigned int UINT32;
typedef unsigned long long UINT64;
typedef long long INT64;
typedef unsigned long long UINTN; // Unsigned integer large enough to hold a pointer
typedef UINT16 CHAR16;            // 16-bit Unicode character
typedef UINT64 EFI_STATUS;        // UEFI status code (64-bit)
typedef void *EFI_HANDLE;         // Opaque handle type
typedef UINT64 EFI_PHYSICAL_ADDRESS;
typedef UINT64 EFI_VIRTUAL_ADDRESS;

// Function pointer type for unloading EFI images
typedef EFI_STATUS(__attribute__((ms_abi)) * EFI_IMAGE_UNLOAD)(EFI_HANDLE ImageHandle);

// ============================================================================
// Status Codes
// ============================================================================
#define EFI_SUCCESS 0                           // Operation completed successfully
#define EFI_BUFFER_TOO_SMALL 0x8000000000000005 // Buffer too small for data
#define EFI_NOT_FOUND 0x800000000000000E        // Requested data not found

// ============================================================================
// File Mode Constants
// ============================================================================
#define EFI_FILE_MODE_READ 0x0000000000000001   // Read access
#define EFI_FILE_MODE_WRITE 0x0000000000000002  // Write access
#define EFI_FILE_MODE_CREATE 0x8000000000000000 // Create file if not exists

// ============================================================================
// GUID Definitions (Globally Unique Identifiers)
// ============================================================================
typedef struct
{
    UINT32 Data1;
    UINT16 Data2;
    UINT16 Data3;
    UINT8 Data4[8];
} EFI_GUID;

// Protocol GUIDs for UEFI services
#define EFI_LOADED_IMAGE_PROTOCOL_GUID \
    {0x5B1B31A1, 0x9562, 0x11d2, {0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B}}

#define EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID \
    {0x964E5B22, 0x6459, 0x11D2, {0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B}}

#define EFI_GRAPHICS_OUTPUT_PROTOCOL_GUID \
    {0x9042A9DE, 0x23DC, 0x4A38, {0x96, 0xFB, 0x7A, 0xDE, 0xD0, 0x80, 0x51, 0x6A}}

#define EFI_FILE_INFO_ID \
    {0x09576E92, 0x6D3F, 0x11D2, {0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B}}

// ============================================================================
// UEFI Table Header
// Common header for all UEFI configuration tables
// ============================================================================
typedef struct
{
    UINT64 Signature;  // Table signature
    UINT32 Revision;   // Table revision
    UINT32 HeaderSize; // Size of the header
    UINT32 CRC32;      // CRC32 checksum of the table
    UINT32 Reserved;   // Reserved field
} EFI_TABLE_HEADER;

// ============================================================================
// Simple Text Output Protocol
// Used for console text output
// ============================================================================
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

// ============================================================================
// Device Path Protocol
// Represents a device path in UEFI
// ============================================================================
typedef struct
{
    UINT8 Type;
    UINT8 SubType;
    UINT16 Length;
} EFI_DEVICE_PATH_PROTOCOL;

// ============================================================================
// Memory Types
// Defines different types of memory regions
// ============================================================================
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

// ============================================================================
// Memory Allocation Types
// ============================================================================
typedef enum
{
    AllocateAnyPages,
    AllocateMaxAddress,
    AllocateAddress,
    MaxAllocateType
} EFI_ALLOCATE_TYPE;

// ============================================================================
// Memory Descriptor
// Describes a memory region
// ============================================================================
typedef struct
{
    UINT32 Type;
    EFI_PHYSICAL_ADDRESS PhysicalStart;
    EFI_VIRTUAL_ADDRESS VirtualStart;
    UINT64 NumberOfPages;
    UINT64 Attribute;
} EFI_MEMORY_DESCRIPTOR;

// ============================================================================
// Graphics Output Protocol (GOP)
// Used for graphics output in UEFI
// ============================================================================
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
    EFI_PHYSICAL_ADDRESS FrameBufferBase; // Physical address of video memory
    UINTN FrameBufferSize;                // Size of frame buffer in bytes
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

// ============================================================================
// File Protocol
// Used for file operations
// ============================================================================
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

// ============================================================================
// File Information Structure
// ============================================================================
typedef struct
{
    UINT64 Size;
    UINT64 FileSize; // Size of file on disk
    UINT64 PhysicalSize;
    void *CreateTime; // Simplified
    void *LastAccessTime;
    void *ModificationTime;
    UINT64 Attribute;
    CHAR16 FileName[1]; // Dynamic filename in UTF-16
} EFI_FILE_INFO;

// ============================================================================
// Simple File System Protocol
// ============================================================================
typedef struct EFI_SIMPLE_FILE_SYSTEM_PROTOCOL
{
    UINT64 Revision;
    EFI_STATUS(__attribute__((ms_abi)) * OpenVolume)(
        struct EFI_SIMPLE_FILE_SYSTEM_PROTOCOL *This,
        EFI_FILE_PROTOCOL **Root);
} EFI_SIMPLE_FILE_SYSTEM_PROTOCOL;

// ============================================================================
// Loaded Image Protocol
// Information about a loaded EFI image
// ============================================================================
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

// ============================================================================
// Boot Services
// UEFI boot services table - available until ExitBootServices()
// ============================================================================
typedef struct EFI_BOOT_SERVICES
{
    EFI_TABLE_HEADER Hdr;

    // Task Priority Level (TPL) services
    void *RaiseTPL;
    void *RestoreTPL;

    // Memory management services
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

    // Event and timer services
    void *CreateEvent;
    void *SetTimer;
    void *WaitForEvent;
    void *SignalEvent;
    void *CloseEvent;
    void *CheckEvent;

    // Protocol handler services
    void *InstallProtocolInterface;
    void *ReinstallProtocolInterface;
    void *UninstallProtocolInterface;
    EFI_STATUS(__attribute__((ms_abi)) * HandleProtocol)(
        EFI_HANDLE Handle,
        EFI_GUID *Protocol,
        void **Interface);
    void *Reserved;
    void *RegisterProtocolNotify;

    // Handle location services
    void *LocateHandle;
    void *LocateDevicePath;
    void *InstallConfigurationTable;

    // Image services
    void *ImageLoad;
    void *ImageStart;
    void *Exit;
    void *ImageUnload;
    EFI_STATUS(__attribute__((ms_abi)) * ExitBootServices)(
        EFI_HANDLE ImageHandle,
        UINTN MapKey);

    // Miscellaneous services
    void *GetNextMonotonicCount;
    void *Stall;
    void *SetWatchdogTimer;

    // Driver services
    void *ConnectController;
    void *DisconnectController;

    // Protocol open/close services
    void *OpenProtocol;
    void *CloseProtocol;
    void *OpenProtocolInformation;

    // Library services
    void *ProtocolsPerHandle;
    void *LocateHandleBuffer;
    EFI_STATUS(__attribute__((ms_abi)) * LocateProtocol)(
        EFI_GUID *Protocol,
        void *Registration,
        void **Interface);
} EFI_BOOT_SERVICES;

// ============================================================================
// Configuration Table Entry
// Used to locate ACPI tables and other system tables
// ============================================================================
typedef struct
{
    EFI_GUID VendorGuid;
    void *VendorTable;
} EFI_CONFIGURATION_TABLE;

// ============================================================================
// System Table
// Main UEFI system table - entry point to all UEFI services
// ============================================================================
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
    EFI_BOOT_SERVICES *BootServices;
    UINTN NumberOfTableEntries;
    EFI_CONFIGURATION_TABLE *ConfigurationTable;
} EFI_SYSTEM_TABLE;

// ============================================================================
// Error Assertion Macro
// Halts execution if status is not EFI_SUCCESS
// ============================================================================
static inline void ASSERT_EFI(
    EFI_SIMPLE_TEXT_OUTPUT_PROTOCOL *ConOut,
    EFI_STATUS Status,
    const CHAR16 *ErrorMessage)
{
    if (Status != EFI_SUCCESS)
    {
        ConOut->OutputString(ConOut, (CHAR16 *)ErrorMessage);
        while (1)
        {
            __asm__("hlt"); // Halt CPU on fatal error
        }
    }
}

#endif // EFI_H