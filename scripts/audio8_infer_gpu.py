#!/usr/bin/env python3
"""One-shot Audio8 TTS on GPU (or CPU) via Transformers.

Used when Settings → Voce → Audio8 device = cuda.
Leaves the ONNX HTTP service as the recommended CPU path so Ollama can keep the GPU.
"""
from __future__ import annotations

import argparse
import sys


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--text", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--device", default="cuda", choices=["cuda", "cpu"])
    p.add_argument("--model", default="Audio8/Audio8-TTS-Preview-0.6b")
    args = p.parse_args()

    try:
        import soundfile as sf
        import torch
        from transformers import AutoModel, AutoProcessor
    except ImportError as e:
        print(
            "Dipendenze mancanti. Installa:\n"
            '  pip install "torch>=2.5.0" "torchaudio>=2.5.0" '
            '"transformers>=4.57.0,<5" "soundfile>=0.12" "safetensors>=0.4"',
            file=sys.stderr,
        )
        print(e, file=sys.stderr)
        return 2

    want = args.device
    if want == "cuda" and not torch.cuda.is_available():
        print("CUDA non disponibile — fallback CPU", file=sys.stderr)
        want = "cpu"

    dtype = torch.bfloat16 if want == "cuda" else torch.float32
    print(f"Loading {args.model} on {want}…", file=sys.stderr)

    processor = AutoProcessor.from_pretrained(args.model, trust_remote_code=True)
    model = (
        AutoModel.from_pretrained(args.model, trust_remote_code=True, dtype=dtype)
        .eval()
        .to(want)
    )

    inputs = processor(text=[args.text], return_tensors="pt")
    inputs = {k: v.to(want) for k, v in inputs.items()}

    with torch.inference_mode():
        output = model.generate(
            **inputs,
            max_new_tokens=1024,
            temperature=0.8,
            top_p=0.95,
            top_k=50,
            do_sample=True,
            return_dict_in_generate=True,
        )
        waveforms, waveform_lengths = model.decode_audio(output.codes)

    audio = waveforms[0, : int(waveform_lengths[0])].float().cpu().numpy()
    sf.write(args.out, audio, model.config.codec_sample_rate)
    print(args.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
