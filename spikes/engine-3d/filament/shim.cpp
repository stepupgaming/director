// Filament 1.80 shim for the issue 9 engine spike.
//
// Filament creates the Vulkan device and renders spike.glb into a headless
// swapchain. A VulkanPlatform subclass records the swapchain VkImages and the
// index of the image each frame used. The images carry no SAMPLED usage, so the
// Rust side copies the finished image on the GPU into a Skia texture on the
// same VkDevice and VkQueue, with no readback.

#include <filament/Camera.h>
#include <filament/ColorGrading.h>
#include <filament/Engine.h>
#include <filament/IndirectLight.h>
#include <filament/LightManager.h>
#include <filament/Options.h>
#include <filament/RenderableManager.h>
#include <filament/Renderer.h>
#include <filament/Scene.h>
#include <filament/Skybox.h>
#include <filament/SwapChain.h>
#include <filament/ToneMapper.h>
#include <filament/View.h>
#include <filament/Viewport.h>

#include <backend/platforms/VulkanPlatformWindows.h>

#include <gltfio/Animator.h>
#include <gltfio/AssetLoader.h>
#include <gltfio/FilamentAsset.h>
#include <gltfio/FilamentInstance.h>
#include <gltfio/MaterialProvider.h>
#include <gltfio/ResourceLoader.h>
#include <gltfio/TextureProvider.h>
#include <gltfio/materials/uberarchive.h>

#include <image/Ktx1Bundle.h>
#include <ktxreader/Ktx1Reader.h>

#include <utils/EntityManager.h>

#include <atomic>
#include <cstdio>
#include <fstream>
#include <iterator>
#include <mutex>
#include <string>
#include <vector>

using namespace filament;
using filament::backend::VulkanPlatformWindows;
using namespace filament::gltfio;

namespace {

class SpikePlatform final : public VulkanPlatformWindows {
public:
    SwapChainBundle getSwapChainBundle(SwapChainPtr handle) override {
        SwapChainBundle bundle = VulkanPlatformWindows::getSwapChainBundle(handle);
        std::lock_guard<std::mutex> lock(mutex);
        images.clear();
        for (VkImage image : bundle.colors) {
            images.push_back(image);
        }
        format = bundle.colorFormat;
        return bundle;
    }

    VkResult acquire(SwapChainPtr handle, ImageSyncData* out) override {
        VkResult result = VulkanPlatformWindows::acquire(handle, out);
        index.store(out->imageIndex);
        return result;
    }

    std::mutex mutex;
    std::vector<VkImage> images;
    VkFormat format = VK_FORMAT_UNDEFINED;
    std::atomic<uint32_t> index{0};
};

std::vector<uint8_t> readFile(const char* path) {
    std::ifstream in(path, std::ios::binary);
    return std::vector<uint8_t>(std::istreambuf_iterator<char>(in), {});
}

bool readSh(const char* path, math::float3 out[9]) {
    std::ifstream in(path);
    std::string line;
    int n = 0;
    while (n < 9 && std::getline(in, line)) {
        float r, g, b;
        if (std::sscanf(line.c_str(), " ( %f , %f , %f )", &r, &g, &b) == 3) {
            out[n++] = {r, g, b};
        }
    }
    return n == 9;
}

struct Spike {
    SpikePlatform platform;
    Engine* engine = nullptr;
    filament::SwapChain* swapChain = nullptr;
    Renderer* renderer = nullptr;
    Scene* scene = nullptr;
    View* view = nullptr;
    MaterialProvider* materials = nullptr;
    AssetLoader* loader = nullptr;
    FilamentAsset* asset = nullptr;
    Animator* animator = nullptr;
    ColorGrading* grading = nullptr;
    AgxToneMapper toneMapper;
    uint32_t skipped = 0;
};

} // namespace

extern "C" {

struct FilInfo {
    uint64_t instance;
    uint64_t physical;
    uint64_t device;
    uint64_t queue;
    uint32_t family;
    uint32_t queueIndex;
    uint32_t format;
    uint32_t imageCount;
    uint64_t images[8];
};

void* fil_create(const char* glb, const char* ibl, const char* sky, const char* shPath,
        uint32_t width, uint32_t height, FilInfo* info) {
    auto* s = new Spike();
    s->engine = Engine::Builder()
            .backend(Engine::Backend::VULKAN)
            .platform(&s->platform)
            .build();
    if (!s->engine) {
        std::fprintf(stderr, "filament: no Vulkan engine\n");
        return nullptr;
    }
    Engine& engine = *s->engine;
    s->swapChain = engine.createSwapChain(width, height, 0);
    s->renderer = engine.createRenderer();
    s->scene = engine.createScene();
    s->view = engine.createView();
    s->view->setScene(s->scene);
    s->view->setViewport({0, 0, width, height});

    // glTF through the prebuilt ubershader archive.
    s->materials = createUbershaderProvider(&engine, UBERARCHIVE_DEFAULT_DATA,
            UBERARCHIVE_DEFAULT_SIZE);
    s->loader = AssetLoader::create({&engine, s->materials, nullptr});
    std::vector<uint8_t> bytes = readFile(glb);
    s->asset = s->loader->createAsset(bytes.data(), (uint32_t) bytes.size());
    if (!s->asset) {
        std::fprintf(stderr, "filament: could not parse %s\n", glb);
        return nullptr;
    }
    ResourceConfiguration rc{};
    rc.engine = &engine;
    rc.normalizeSkinningWeights = true;
    ResourceLoader resources(rc);
    TextureProvider* stb = createStbProvider(&engine);
    resources.addTextureProvider("image/png", stb);
    resources.addTextureProvider("image/jpeg", stb);
    resources.loadResources(s->asset);
    s->asset->releaseSourceData();
    s->scene->addEntities(s->asset->getEntities(), s->asset->getEntityCount());
    s->animator = s->asset->getInstance()->getAnimator();

    auto& rm = engine.getRenderableManager();
    for (size_t i = 0; i < s->asset->getRenderableEntityCount(); i++) {
        auto ri = rm.getInstance(s->asset->getRenderableEntities()[i]);
        rm.setCastShadows(ri, true);
        rm.setReceiveShadows(ri, true);
    }
    auto& lm = engine.getLightManager();
    for (size_t i = 0; i < s->asset->getLightEntityCount(); i++) {
        auto li = lm.getInstance(s->asset->getLightEntities()[i]);
        lm.setShadowCaster(li, true);
    }

    // Camera from the glTF, exposure matched to the Bevy spike (EV100 about 11).
    if (s->asset->getCameraEntityCount() == 0) {
        std::fprintf(stderr, "filament: glTF has no camera\n");
        return nullptr;
    }
    Camera* camera = engine.getCameraComponent(s->asset->getCameraEntities()[0]);
    camera->setExposure(4.0f, 1.0f / 125.0f, 100.0f);
    s->view->setCamera(camera);

    // IBL from cmgen output.
    math::float3 sh[9];
    if (!readSh(shPath, sh)) {
        std::fprintf(stderr, "filament: bad %s\n", shPath);
        return nullptr;
    }
    std::vector<uint8_t> iblBytes = readFile(ibl);
    std::vector<uint8_t> skyBytes = readFile(sky);
    auto* iblBundle = new image::Ktx1Bundle(iblBytes.data(), (uint32_t) iblBytes.size());
    auto* skyBundle = new image::Ktx1Bundle(skyBytes.data(), (uint32_t) skyBytes.size());
    Texture* iblTexture = ktxreader::Ktx1Reader::createTexture(&engine, iblBundle, false);
    Texture* skyTexture = ktxreader::Ktx1Reader::createTexture(&engine, skyBundle, false);
    s->scene->setIndirectLight(IndirectLight::Builder()
            .reflections(iblTexture)
            .irradiance(3, sh)
            .intensity(3000.0f)
            .build(engine));
    s->scene->setSkybox(Skybox::Builder().environment(skyTexture).intensity(3000.0f).build(engine));

    // Post: bloom, FXAA, AgX tone mapping, screen-space refraction for the glass.
    View::BloomOptions bloom;
    bloom.enabled = true;
    s->view->setBloomOptions(bloom);
    s->view->setAntiAliasing(View::AntiAliasing::FXAA);
    s->view->setScreenSpaceRefractionEnabled(true);
    s->grading = ColorGrading::Builder().toneMapper(&s->toneMapper).build(engine);
    s->view->setColorGrading(s->grading);

    Renderer::ClearOptions clear;
    clear.clear = true;
    s->renderer->setClearOptions(clear);

    engine.flushAndWait();

    info->instance = (uint64_t) s->platform.getInstance();
    info->physical = (uint64_t) s->platform.getPhysicalDevice();
    info->device = (uint64_t) s->platform.getDevice();
    info->queue = (uint64_t) s->platform.getGraphicsQueue();
    info->family = s->platform.getGraphicsQueueFamilyIndex();
    info->queueIndex = s->platform.getGraphicsQueueIndex();
    {
        std::lock_guard<std::mutex> lock(s->platform.mutex);
        info->format = (uint32_t) s->platform.format;
        info->imageCount = (uint32_t) s->platform.images.size();
        for (size_t i = 0; i < s->platform.images.size() && i < 8; i++) {
            info->images[i] = (uint64_t) s->platform.images[i];
        }
    }
    return s;
}

// Renders one frame at time t and waits for the GPU. Returns the swapchain
// image index the frame landed in, or -1 when Filament skipped the frame.
int fil_frame(void* handle, float t) {
    auto* s = (Spike*) handle;
    s->animator->applyAnimation(0, t);
    s->animator->updateBoneMatrices();
    int index = -1;
    if (s->renderer->beginFrame(s->swapChain)) {
        s->renderer->render(s->view);
        s->renderer->endFrame();
        index = (int) s->platform.index.load();
    } else {
        s->skipped++;
    }
    s->engine->flushAndWait();
    return index;
}

uint32_t fil_skipped(void* handle) {
    return ((Spike*) handle)->skipped;
}

} // extern "C"
