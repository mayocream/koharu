import torch

class SumModule(torch.nn.Module):
    def forward(self, x):
        return torch.sum(x, dim=1)


torch.onnx.export(
    SumModule(),
    (torch.ones(2, 2, 2),),
    "onnx.pb",
    input_names=["x"],
    output_names=["sum"],
    dynamic_axes={
        # dict value: manually named axes
        "x": {0: "batch"},
        # list value: automatic names
        "sum": {0: "batch"},
    },
)

import numpy as np
import onnxruntime as ort

def batch_inference_with_onnx(batch_input, model_path="onnx.pb"):
    """
    Perform batch inference using an ONNX model with all inputs processed at once

    Args:
        batch_input: Numpy array with batch dimension as the first dimension
        model_path: Path to the ONNX model file

    Returns:
        Inference results for the entire batch
    """
    # Create an ONNX Runtime session
    session = ort.InferenceSession(model_path)

    # Get input and output names
    input_name = session.get_inputs()[0].name  # Should be "x" based on export
    output_name = session.get_outputs()[0].name  # Should be "sum" based on export

    print(batch_input)

    # Ensure input is numpy array with correct type
    if not isinstance(batch_input, np.ndarray):
        batch_input = np.stack(batch_input, axis=0)
    else:
        batch_input = batch_input.astype(np.float32)

    print("input ", batch_input)

    # Run inference on the entire batch at once
    outputs = session.run(None, {input_name: batch_input})

    return outputs[0]

# Example usage
if __name__ == "__main__":
    # Create some sample batches - varying the batch size to demonstrate dynamic axes
    batch1 = np.ones((2, 2), dtype=np.float32)
    batch2 = np.ones((2, 2), dtype=np.float32)/2
    batch3 = np.ones((2, 2), dtype=np.float32)/3

    # Process individual batches
    # result1 = batch_inference_with_onnx(batch1)
    # print(f"Batch 1 shape: {batch1.shape}, Result shape: {result1}")

    # Process multiple batches at once
    results = batch_inference_with_onnx([batch1, batch2, batch3])

    for i, result in enumerate(results):
        print(f"Batch {i+1} result shape: {results[i]}")

    # For production use, you might want to add optimizations:
    # session_options = ort.SessionOptions()
    # session_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
    # session_options.intra_op_num_threads = 4  # Parallel threads
    # session = ort.InferenceSession(model_path, session_options)
