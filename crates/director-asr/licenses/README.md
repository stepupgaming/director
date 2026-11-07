# Third-party licenses for speech to text

Director ships no model weights. `director-engine asr fetch` downloads them from Hugging Face at the revisions pinned in `src/store.rs`, and these terms apply to the downloaded files.

| Component | Used as | License | Files |
| --- | --- | --- | --- |
| Underdog Bark 3.0, Plus, Clean and Pro (Conway Research) | model weights, fetched at run time from the revisions below | CC BY 4.0 | `CC-BY-4.0.txt`, `underdog-bark-3.0*.NOTICE`, `underdog-bark-3.0*.card.md` |
| nvidia/parakeet-tdt-0.6b-v3 (NVIDIA) and moondream/parakeet-ultra (Moondream) | upstream of the Bark 3.0 models and the `bark-asr` graphs | CC BY 4.0 | `CC-BY-4.0.txt` |
| `bark-asr` 0.1.0 graphs (network structure without weights, vocab) | fetched at run time from the `bark-rs` repository | CC BY 4.0 | `bark-asr.NOTICE`, `CC-BY-4.0.txt` |
| `bark-asr` 0.1.0 code | Rust dependency | MIT OR Apache-2.0 | `bark-asr.LICENSE-MIT`, `../../../LICENSE-APACHE` |
| `ort` 2.0.0-rc.15 (pyke.io) | Rust dependency | MIT OR Apache-2.0 | `ort.LICENSE-MIT`, `../../../LICENSE-APACHE` |
| ONNX Runtime 1.31.0 (Microsoft) | linked statically through `ort` | MIT, with its own third-party notices | `onnxruntime.LICENSE`, `onnxruntime-ThirdPartyNotices.txt` |

## Bark 3.0 revisions

Conway Research released the four models under CC BY 4.0 on 2026-10-15. On 2026-11-05 at 04:43 to 04:44 UTC each repository received a commit titled "Switch the license to CC BY-NC 4.0" that changed the license files and model card and left the weight files byte-identical. Director pins the last CC BY 4.0 revision of each repository, where the model card's `license` field reads `cc-by-4.0` and the NOTICE links the CC BY 4.0 license. The `.NOTICE` and `.card.md` files here are verbatim copies from those revisions.

| Model | Pinned revision (CC BY 4.0) | Weight file, size, SHA-256 | Switch commit (CC BY-NC 4.0) |
| --- | --- | --- | --- |
| [Underdog Bark 3.0](https://huggingface.co/ConwayResearch/Underdog-Bark-3.0/tree/85db2ad18ab41e534a222ee253c29191d77ed919) | `85db2ad18ab41e534a222ee253c29191d77ed919` | `model.bark`, 163398645, `c18de9418d73cbeb80b8c1287b392d480a56e191ef4a81d1baaa1b02e66d92ed` | `e7a641c0fdd4cdf1818dde62a98c26e11d65c061` |
| [Underdog Bark 3.0 Plus](https://huggingface.co/ConwayResearch/Underdog-Bark-3.0-Plus/tree/d4fab2f8b95681e5ad3cfb4e8c5f4e0c009b3deb) | `d4fab2f8b95681e5ad3cfb4e8c5f4e0c009b3deb` | `model.bark`, 179524845, `fdd9f1052e4fdead07d71e8cd2138813eb0c209fd6ff8b1b6f47737e1e501857` | `28a7d4234146b0f01c11e0c6b72dfa29028a4758` |
| [Underdog Bark 3.0 Clean](https://huggingface.co/ConwayResearch/Underdog-Bark-3.0-Clean/tree/6d34ed8639b2b9a33e764acf06748519bbb950cf) | `6d34ed8639b2b9a33e764acf06748519bbb950cf` | `model.bark`, 179458613, `b67ca90d94b79a36af4616f1cb3f241884925de7aa9f3e673a8d5c6c4cfd3436` | `73775471a9410b75ceb2401e837861dac12bd76d` |
| [Underdog Bark 3.0 Pro](https://huggingface.co/ConwayResearch/Underdog-Bark-3.0-Pro/tree/4b5c51e04aa8d16dda9ecdff71bc29f7360149e4) | `4b5c51e04aa8d16dda9ecdff71bc29f7360149e4` | `model.safetensors`, 1254196636, `6f84b33e63ebd1851e910e8b2fe299d29f5e0efc6f57e84d92d2f79af464e0e5` | `d87f288a9a0b0cdb92228057e168ce575b300a6b` |

Section 2(a)(1) of CC BY 4.0 grants the license as "irrevocable". Attribution for redistribution: the NOTICE of each model names Conway Research, NVIDIA and Moondream.

On Windows, ONNX Runtime's DirectML provider loads `DirectML.dll` from the system; Director does not ship it.
