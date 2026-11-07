---
license: cc-by-4.0
language:
- en
library_name: transformers
pipeline_tag: automatic-speech-recognition
base_model: nvidia/parakeet-tdt-0.6b-v3
base_model_relation: finetune
tags:
- underdog
- speech-to-text
- asr
- stt
- parakeet
- tdt
metrics:
- wer
---

# Underdog Bark 3.0 Pro

Bark 3.0 Pro is state-of-the-art open speech recognition: a 0.6B English model at full precision (bf16, 1.25 GB). Across the Open ASR Leaderboard's seven English sets it averages 4.29 % word error, the lowest of any open model by the leaderboard's own scoring: 4.286 against 4.291 for Hojo-ASR-V1, a 12 GB download. Every other open model is further behind, including Qwen3-ASR-1.7B (4.43 %) and Canary-Qwen 2.5B (4.55 %).

It has the lowest error of any open model on the leaderboard for parliamentary speech (1.70 % on VoxPopuli) and earnings calls (5.65 % on Earnings-22), and 7.16 % on meetings (AMI). It loads as a stock `transformers` model with no custom code.

![Accuracy vs download size](bark_frontier.png)

## In Underdog

[Underdog](https://underdog.ai)'s native Swift runtime loads `model.safetensors` directly on Apple silicon with MLX, with no Python, and keeps the weights in bf16 as published. It reads the vocabulary from `mlx/config.json`, which is the same file the smaller Bark 3.0 models ship. Your audio never leaves the device.

## Highlights

- State of the art: the most accurate open-weight speech model on the Open ASR Leaderboard's seven English sets by its own scoring: 4.286 %, just ahead of Hojo-ASR-V1 (4.291 %) from about a tenth of the download.
- More accurate than every other open model, including Qwen3-ASR-1.7B (4.43 %), higgs-audio-v3-stt (4.45 %), Canary-Qwen 2.5B (4.55 %), Granite Speech 4.1 2B (4.69 %), Cohere Transcribe (4.85 %), Voxtral Small 24B (5.21 %) and Whisper large-v3 (6.04 %).
- More accurate than paid speech APIs including OpenAI GPT-Transcribe (4.70 %), AssemblyAI Universal-3.5 Pro (4.46 %), Gladia Solaria-3 (4.70 %), Smallest.ai Pulse (4.54 %) and Meta Muse Voice Transcribe (4.93 %).
- The Bark 3.0 family holds the accuracy frontier for open speech recognition at every size from 163 MB to 1.25 GB.
- The lowest error of any open model on parliamentary speech (1.70 % on VoxPopuli) and earnings calls (5.65 % on Earnings-22).


## Benchmarks

| Model | Download | LS clean | LS other | AMI | Earnings-22 | GigaSpeech | SPGISpeech | VoxPopuli | Average |
|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| **Bark 3.0 Pro** | **1,254 MB** | 1.38 | 3.11 | 7.16 | **5.65** | 7.46 | 3.54 | **1.70** | **4.29** |
| Bark 3.0 Plus | 180 MB | 1.71 | 3.82 | 7.76 | 6.70 | 7.77 | 3.64 | 1.92 | 4.76 |
| Bark 3.0 | 163 MB | 1.89 | 4.08 | 7.82 | 7.05 | 7.95 | 3.72 | 2.05 | 4.94 |
| Phonon-2† | 164 MB | 1.73 | 3.92 | 9.32 | 6.96 | 8.35 | 3.70 | 2.46 | 5.21 |
| Hojo-ASR-V1† | 11,957 MB | 1.30 | 3.24 | 7.54 | 6.97 | **6.14** | 1.74 | 3.11 | 4.29 |
| Qwen3-ASR-1.7B† | 4,699 MB | 1.26 | 2.94 | 8.31 | 5.84 | 7.22 | 2.57 | 2.85 | 4.43 |
| higgs-audio-v3-stt† | 5,351 MB | 1.08 | 2.61 | 7.19 | 7.04 | 7.45 | 2.07 | 3.73 | 4.45 |
| Canary-Qwen 2.5B† | 5,119 MB | 1.23 | 2.63 | 7.91 | 6.41 | 7.80 | **1.70** | 4.14 | 4.55 |
| Granite Speech 4.1 2B† | 4,832 MB | 1.03 | 2.19 | 7.06 | 6.69 | 8.18 | 3.47 | 4.18 | 4.69 |
| Cohere Transcribe† | 4,132 MB | **0.97** | **2.05** | **7.02** | 7.88 | 7.88 | 2.74 | 5.38 | 4.85 |
| Granite Speech 5.0 470M (NC)† | 946 MB | 1.45 | 2.67 | 7.31 | 8.23 | 8.37 | 1.75 | 4.67 | 4.92 |
| Parakeet TDT 0.6B v3† | 2,508 MB | 1.52 | 3.13 | 9.42 | 5.85 | 7.99 | 3.63 | 3.05 | 4.94 |
| Granite Speech 5.0 470M† | 946 MB | 1.48 | 2.71 | 7.71 | 6.75 | 8.74 | 3.22 | 4.90 | 5.07 |
| Qwen3-ASR-0.6B† | 1,876 MB | 1.70 | 4.01 | 9.33 | 7.83 | 7.62 | 2.74 | 2.90 | 5.16 |
| Whisper large-v3† | 9,261 MB | 1.56 | 3.52 | 13.63 | 7.99 | 8.35 | 2.71 | 4.53 | 6.04 |
| Whisper large-v3-turbo† | 1,618 MB | 2.13 | 3.71 | 13.88 | 8.09 | 8.47 | 2.79 | 7.02 | 6.58 |

Word error rate (%), lower is better, on the full test sets of the Open ASR Leaderboard's seven English sets (cleaned versions of AMI, Earnings-22, GigaSpeech and VoxPopuli). † The leaderboard's published row, as of 9 October 2026; the Bark rows use its scoring code and are not on the leaderboard yet. Bold marks the best value in each column. At three decimals, by the leaderboard's own averaging, Bark 3.0 Pro averages 4.286 and Hojo-ASR-V1 4.291. NC: non-commercial licence. Download is the size of the weight files on Hugging Face.


### Disfluent speech

On the DisfluencySpeech test set (250 clips of unscripted conversation full of "um", "uh", restarts and repeats), scored against its verbatim transcripts with fillers counted:

| Model | Word error rate (%) |
|---|--:|
| Bark 3.0 Pro | **2.95** |
| Bark 3.0 | 3.15 |
| Bark 3.0 Plus | 3.21 |
| Phonon-2 | 3.39 |

Bark 3.0 writes what was said, fillers included. It does not clean speech up.

## Run it

Choose one runtime below. Windows CUDA uses the original BF16 Safetensors weights at the repository root. Mac Core ML uses the separate `coreml/` model files; GPU and ANE share those files. Selective downloads avoid downloading both formats.

### Mac: Core ML GPU or ANE

Requires an Apple Silicon Mac, macOS 15 or later, and Swift 6.2 or later (Xcode / Command Line Tools). The runner uses the GPU or ANE for supported encoder operations and the CPU for preprocessing and decoding. Both paths were tested on an M5 Max. For iPhone and iPad, see the iOS section below.

Install the download CLI, then download only the Core ML package and its runner. If repository access requires authentication, run `hf auth login` first.

```bash
python3 -m venv .venv-download
source .venv-download/bin/activate
python -m pip install huggingface_hub==2.2.0
BARK_REVISION=main
hf download ConwayResearch/Underdog-Bark-3.0-Pro --revision "$BARK_REVISION" \
  --local-dir bark-pro \
  --include 'coreml/*' --include 'runtime/coreml/*' --include 'NOTICE'

swift --version
swift build --package-path bark-pro/runtime/coreml -c release
```

Use a mono, 16 kHz WAV file. To convert another input with FFmpeg, if needed:

```bash
ffmpeg -i input.wav -ac 1 -ar 16000 recording-16k.wav
```

Run on the Apple GPU:

```bash
bark-pro/runtime/coreml/.build/release/bark-pro-coreml \
  bark-pro/coreml recording-16k.wav gpu
```

Run the same model with CPU + ANE instead:

```bash
bark-pro/runtime/coreml/.build/release/bark-pro-coreml \
  bark-pro/coreml recording-16k.wav ane
```

The program prints the transcript and exits. It adds the evaluated 0.25 seconds of leading silence and automatically selects the 8-second encoder function for short inputs, otherwise the 15-second path with FluidAudio long-audio handling. GPU/ANE selection is a compute-unit configuration, not a guarantee that every operation runs on that accelerator.

GPU and ANE use the same Core ML model files. The `gpu` option lets Core ML execute supported encoder operations on the Apple GPU; `ane` uses the Apple Neural Engine instead. Preprocessing and decoding stay on the CPU in both modes.

The package shares encoder weights between its 8-second and 15-second functions, so short recordings need less computation without duplicating the full model. Its greedy joint selects the next token without producing unused top-K candidate outputs. Both optimizations are enabled automatically on GPU and ANE; no extra flags are needed.

The Core ML download is approximately 1.23 GB: FP16 learned weights with an FP32 preprocessor. Initial model compilation/loading is additional to inference time. This runner uses greedy decoding without contextual vocabulary biasing or language filtering. Core ML may emit an initialization shape diagnostic from the preprocessor; the tested nonempty, padded inputs completed successfully. Empty input is rejected. The root benchmark table describes the original model's evaluation, not separate measurements of every runtime.

See [Core ML license](coreml/LICENSE) and [attribution](coreml/NOTICE). The Swift dependency is pinned in `runtime/coreml/Package.swift` and `Package.resolved`; its first build requires network access. No Python model runtime is needed after downloading these files.

### iPhone and iPad (iOS)

Requires iOS 18 or later (the encoder's shared 8-second and 15-second functions are a Core ML multifunction model) and a Swift package dependency on [FluidAudio](https://github.com/FluidInference/FluidAudio), pinned to the revision in `runtime/coreml/Package.swift`. The model files are the same as on the Mac; nothing is converted for iOS. **Not yet tested on an iPhone or iPad.**

What an app needs:

- The files in `coreml/` (about 1.23 GB): `Preprocessor`, `Encoder`, `Decoder` and `JointDecisionv3` (`.mlpackage`) and `parakeet_v3_vocab.json`. Add the packages to the Xcode target, which compiles them to `.mlmodelc` at build time, or download them and compile each once on the device with `MLModel.compileModel(at:)`, then keep the compiled copies (the Mac runner deletes its temporary compiled copies after each run; an app should not).
- The model loading and transcription code of `runtime/coreml/Sources/main.swift`, which uses only Core ML, AVFoundation and FluidAudio, all available on iOS. Replace the command-line arguments with app settings: the compute units for the encoder (`.cpuAndNeuralEngine` for the ANE, `.cpuAndGPU` for the GPU), and the folder of the compiled models. Keep the rest as in the runner: preprocessor, decoder and joint on the CPU; 0.25 seconds of leading silence; the `short8` encoder function for inputs up to 8 seconds including that silence, otherwise `full15`; `AsrModels(..., version: .ultra)`, `AsrManager` and `TdtDecoderState`.
- Mono 16 kHz float audio. The runner reads a WAV file in that format; in an app, convert microphone or file audio with `AVAudioConverter` first.
- To save memory and load time, create the models once and reuse them for every request.

### Windows: NVIDIA GPU with PyTorch CUDA

Requires 64-bit Windows, Python 3.11, an NVIDIA GPU, and a driver compatible with the CUDA 12.8 PyTorch wheel. BF16 requires a BF16-capable GPU. This is a PyTorch execution path, not an ONNX or DirectML package. Tested on Windows with an NVIDIA GPU (CUDA, BF16).

In PowerShell:

```powershell
py -3.11 -m venv .venv-bark
.\.venv-bark\Scripts\python.exe -m pip install huggingface_hub==2.2.0
# If access requires authentication:
# .\.venv-bark\Scripts\hf.exe auth login
$BarkRevision = "main"
.\.venv-bark\Scripts\hf.exe download ConwayResearch/Underdog-Bark-3.0-Pro --revision $BarkRevision --local-dir bark-pro --include "model.safetensors" --include "config.json" --include "generation_config.json" --include "processor_config.json" --include "tokenizer.json" --include "tokenizer_config.json" --include "runtime/transcribe_bark_pro.py" --include "runtime/requirements.txt" --include "NOTICE"
.\.venv-bark\Scripts\python.exe -m pip install torch==2.8.0 --index-url https://download.pytorch.org/whl/cu128
.\.venv-bark\Scripts\python.exe -m pip install -r bark-pro/runtime/requirements.txt
.\.venv-bark\Scripts\python.exe -c "import torch; print('CUDA available:', torch.cuda.is_available()); print('GPU:', torch.cuda.get_device_name(0) if torch.cuda.is_available() else 'none')"
.\.venv-bark\Scripts\python.exe bark-pro/runtime/transcribe_bark_pro.py recording.wav --model bark-pro --device cuda --dtype bfloat16
```

The Python runner downmixes/resamples audio to mono 16 kHz and adds 0.25 seconds of leading silence. It loads the processor once and fails explicitly if CUDA is unavailable; it does not silently fall back to CPU. If the NVIDIA GPU lacks BF16 support, `--dtype float16` is an explicit alternative requiring separate accuracy validation. For CPU-only execution, install the CPU PyTorch wheel instead and use `--device cpu --dtype float32`.

The pinned Python runtime dependencies include `librosa`, which the feature extractor requires. Follow [PyTorch's installation instructions](https://pytorch.org/get-started/previous-versions/#v280) if the GPU or driver requires a different wheel. Installing CUDA-enabled PyTorch does not make AMD or Intel GPUs CUDA-compatible.

### Python API

The original Transformers interface remains available with the Python dependencies above:

```python
import numpy as np, soundfile as sf, soxr, torch
from transformers import AutoProcessor, ParakeetForTDT

repo = "ConwayResearch/Underdog-Bark-3.0-Pro"
processor = AutoProcessor.from_pretrained(repo)
model = ParakeetForTDT.from_pretrained(repo, dtype=torch.bfloat16).to("cuda").eval()

wave, sr = sf.read("recording.wav", dtype="float32", always_2d=True)
wave = soxr.resample(wave.mean(1), sr, 16000) if sr != 16000 else wave.mean(1)
wave = np.concatenate([np.zeros(4000, np.float32), wave])
inputs = processor(wave, sampling_rate=16000, return_tensors="pt").to("cuda", torch.bfloat16)
with torch.inference_mode():
    out = model.generate(**inputs)
ids = out.sequences if hasattr(out, "sequences") else out
print(processor.batch_decode(ids, skip_special_tokens=True)[0])
```

For a small download, use Bark 3.0 (163 MB) or Bark 3.0 Plus (180 MB).

## Notes

Based on parakeet-tdt-0.6b-v3 by NVIDIA, through moondream/parakeet-ultra; the tokenizer and output conventions (punctuation, casing, numerals) are the original's. Licence CC-BY-4.0, same as the original; `NOTICE` lists the attribution. English only.
