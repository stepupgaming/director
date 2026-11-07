# Third-party licenses for speech to text

Director ships no model weights. `director-engine asr fetch` downloads them from Hugging Face at the revisions pinned in `src/store.rs`, and these terms apply to the downloaded files.

| Component | Used as | License | Files |
| --- | --- | --- | --- |
| Underdog Bark 3.0, Plus, Clean and Pro (Conway Research) | model weights, fetched at run time | CC BY-NC 4.0; commercial use needs a license from Conway Research (license@conway.tech) | `CC-BY-NC-4.0.txt`, `underdog-bark-3.0*.NOTICE` |
| nvidia/parakeet-tdt-0.6b-v3 (NVIDIA) and moondream/parakeet-ultra (Moondream) | upstream of the Bark 3.0 models and the `bark-asr` graphs | CC BY 4.0 | `CC-BY-4.0.txt` |
| `bark-asr` 0.1.0 graphs (network structure without weights, vocab) | fetched at run time from the `bark-rs` repository | CC BY 4.0 | `bark-asr.NOTICE`, `CC-BY-4.0.txt` |
| `bark-asr` 0.1.0 code | Rust dependency | MIT OR Apache-2.0 | `bark-asr.LICENSE-MIT`, `../../../LICENSE-APACHE` |
| `ort` 2.0.0-rc.15 (pyke.io) | Rust dependency | MIT OR Apache-2.0 | `ort.LICENSE-MIT`, `../../../LICENSE-APACHE` |
| ONNX Runtime 1.31.0 (Microsoft) | linked statically through `ort` | MIT, with its own third-party notices | `onnxruntime.LICENSE`, `onnxruntime-ThirdPartyNotices.txt` |

On Windows, ONNX Runtime's DirectML provider loads `DirectML.dll` from the system; Director does not ship it.
