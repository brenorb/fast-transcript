//! Runnable model presets and recommendations from the checked-in ASR v2 benchmarks.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelRuntime {
    Onnx,
    Photon,
    Phonon,
}

#[derive(Debug)]
pub(crate) struct ModelPreset {
    pub(crate) name: &'static str,
    pub(crate) repository: &'static str,
    pub(crate) url: &'static str,
    /// Photon: Hub revision; Phonon: archive SHA-256 from the benchmark SDK.
    pub(crate) revision: Option<&'static str>,
    pub(crate) runtime: ModelRuntime,
    pub(crate) advice: &'static str,
}

pub(crate) const MODELS: [ModelPreset; 4] = [
    ModelPreset {
        name: "parakeet-v3-int8",
        repository: "brenorb/parakeet-tdt-0.6b-v3-int8-onnx-bundle",
        url: crate::DEFAULT_MODEL_URL,
        revision: None,
        runtime: ModelRuntime::Onnx,
        advice: "Default, native CPU. Best 15s Portuguese WER (0%); quickest first result (0.83s). Cache 640 MiB.",
    },
    ModelPreset {
        name: "parakeet-redux",
        repository: "moondream/parakeet-redux",
        url: "https://huggingface.co/moondream/parakeet-redux",
        revision: Some("af60db939ebab3ca8b95b5983174e669599a2352"),
        runtime: ModelRuntime::Photon,
        advice: "Smallest model cache (171 MiB). CPU or GPU; English TED WER 12.11% on MPS.",
    },
    ModelPreset {
        name: "parakeet-ultra",
        repository: "moondream/parakeet-ultra",
        url: "https://huggingface.co/moondream/parakeet-ultra",
        revision: Some("510e6f5a1c4619f39c72b083c091476935734e65"),
        runtime: ModelRuntime::Photon,
        advice: "Accuracy choice for our English and reviewed PT lecture cases: WER 9.72% TED, 0.76% LibriSpeech, 4.05% lecture (MPS). Cache 1.17 GiB.",
    },
    ModelPreset {
        name: "phonon-2",
        repository: "FermionResearch/Phonon-2",
        url: "https://huggingface.co/FermionResearch/Phonon-2/resolve/main/phonon-2.bps.tar.zst",
        revision: Some("98125795b6dda72f5c6eee9ba33d19815df65dcb18b50a357bf9f73c9935309e"),
        runtime: ModelRuntime::Phonon,
        advice: "Fastest loaded GPU inference in our cases (331.5x English TED, MLX), with higher lecture WER (15.89% PT). Cache 325 MiB.",
    },
];

pub(crate) fn find_model(name: &str) -> Option<&'static ModelPreset> {
    let name = match name {
        "default" | "onnx" => "parakeet-v3-int8",
        "redux" => "parakeet-redux",
        "ultra" => "parakeet-ultra",
        "phonon" => "phonon-2",
        name => name,
    };
    MODELS.iter().find(|model| model.name == name)
}

pub(crate) fn list_models() -> String {
    let mut lines = vec!["Models: fscript <audio> --model NAME".to_string()];
    for model in &MODELS {
        lines.push(format!("\n  {}\n    {}", model.name, model.advice));
    }
    lines.extend([
        String::new(),
        "Aliases: default/onnx, redux, ultra, phonon.".to_string(),
        "Device: --device auto|cpu|mps|mlx|cuda (auto selects an available runtime).".to_string(),
        "Redux, Ultra and Phonon use pinned Python SDKs, installed automatically with uv on first use.".to_string(),
        String::new(),
        "Evidence: ASR v2, 2026-09-30, Apple M5 Max; native SDK workers, shared audio partitions.".to_string(),
        "WER is lower-is-better. These recordings do not establish a universal ranking.".to_string(),
        "PT lecture reference began as an Ultra draft reviewed by the user; PT TEDx uses automatic captions.".to_string(),
        "ONNX had the lowest PT TEDx caption disagreement (20.49%); that is not a human accuracy score.".to_string(),
        "Reports: experiments/transcription_benchmarks/README.md".to_string(),
    ]);
    lines.join("\n")
}
