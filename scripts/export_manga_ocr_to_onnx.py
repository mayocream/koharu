import torch
from transformers import VisionEncoderDecoderModel

model = VisionEncoderDecoderModel.from_pretrained("kha-white/manga-ocr-base")
model.eval()

# Dummy input for the model
dummy_image = torch.randn(1, 3, 224, 224)
dummy_token_ids = torch.tensor([[2]])

# Export the model
torch.onnx.export(
    model,
    (dummy_image, dummy_token_ids),
    "models/manga-ocr.onnx",
    input_names=["image", "token_ids"],
    output_names=["logits"],
    dynamic_axes={
        "image": {
            0: "batch_size",
        },
        "token_ids": {
            0: "batch_size",
            1: "sequence_length",
        },
        "logits": {
            0: "batch_size",
            1: "sequence_length",
        },
    },
)
