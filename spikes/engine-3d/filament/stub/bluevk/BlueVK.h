// Minimal stand-in for Filament's bluevk/BlueVK.h, which the prebuilt SDK
// does not ship. It declares only the Vulkan types VulkanPlatform.h names,
// with Vulkan's ABI: handles are pointers, enums and flags are 32-bit, and
// VkDeviceSize is 64-bit. Structs passed by reference stay incomplete.
#pragma once
#include <stdint.h>

#define VK_NULL_HANDLE nullptr
#define VK_STUB_HANDLE(name) typedef struct name##_T* name

VK_STUB_HANDLE(VkInstance);
VK_STUB_HANDLE(VkPhysicalDevice);
VK_STUB_HANDLE(VkDevice);
VK_STUB_HANDLE(VkQueue);
VK_STUB_HANDLE(VkImage);
VK_STUB_HANDLE(VkBuffer);
VK_STUB_HANDLE(VkDeviceMemory);
VK_STUB_HANDLE(VkSemaphore);
VK_STUB_HANDLE(VkSurfaceKHR);

typedef uint64_t VkDeviceSize;
typedef uint32_t VkFlags;
typedef VkFlags VkImageUsageFlags;

typedef enum VkResult { VK_SUCCESS = 0, VK_RESULT_MAX_ENUM = 0x7FFFFFFF } VkResult;
typedef enum VkFormat {
    VK_FORMAT_UNDEFINED = 0,
    VK_FORMAT_R8G8B8A8_UNORM = 37,
    VK_FORMAT_R8G8B8A8_SRGB = 43,
    VK_FORMAT_B8G8R8A8_UNORM = 44,
    VK_FORMAT_MAX_ENUM = 0x7FFFFFFF
} VkFormat;
typedef enum VkSampleCountFlagBits { VK_SAMPLE_COUNT_1_BIT = 1, VK_SAMPLE_COUNT_MAX_ENUM = 0x7FFFFFFF } VkSampleCountFlagBits;
typedef enum VkSamplerYcbcrModelConversion { VK_SAMPLER_YCBCR_MODEL_CONVERSION_MAX_ENUM = 0x7FFFFFFF } VkSamplerYcbcrModelConversion;
typedef enum VkSamplerYcbcrRange { VK_SAMPLER_YCBCR_RANGE_MAX_ENUM = 0x7FFFFFFF } VkSamplerYcbcrRange;
typedef enum VkChromaLocation { VK_CHROMA_LOCATION_MAX_ENUM = 0x7FFFFFFF } VkChromaLocation;
typedef enum VkComponentSwizzle { VK_COMPONENT_SWIZZLE_MAX_ENUM = 0x7FFFFFFF } VkComponentSwizzle;
typedef enum VkExternalFenceHandleTypeFlagBits { VK_EXTERNAL_FENCE_HANDLE_TYPE_FLAG_BITS_MAX_ENUM = 0x7FFFFFFF } VkExternalFenceHandleTypeFlagBits;

typedef struct VkExtent2D { uint32_t width; uint32_t height; } VkExtent2D;
typedef struct VkComponentMapping {
    VkComponentSwizzle r, g, b, a;
} VkComponentMapping;

struct VkInstanceCreateInfo;
struct VkDeviceCreateInfo;
struct VkPhysicalDeviceFeatures;
struct VkPhysicalDeviceProperties;
struct VkPhysicalDeviceVulkan11Features;
