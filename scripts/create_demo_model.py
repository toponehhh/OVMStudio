"""Create a tiny, deterministic ONNX Add model using only Python's standard library.

The generated model implements output = input + input for float32 shape [1, 3].
No model weights, network downloads, or Python packages are required.
"""
from pathlib import Path


def varint(n):
    data = bytearray()
    while n > 127:
        data.append((n & 127) | 128)
        n >>= 7
    data.append(n)
    return bytes(data)


def integer(field, n):
    return varint(field << 3) + varint(n)


def message(field, data):
    if isinstance(data, str):
        data = data.encode("utf-8")
    return varint((field << 3) | 2) + varint(len(data)) + data


def value_info(name):
    # ValueInfoProto -> TypeProto -> Tensor -> float32 + TensorShapeProto.
    shape = message(1, integer(1, 1)) + message(1, integer(1, 3))
    tensor_type = integer(1, 1) + message(2, shape)
    return message(1, name) + message(2, message(1, tensor_type))


def create_model(destination):
    node = message(1, "input") * 2 + message(2, "output") + message(3, "double") + message(4, "Add")
    graph = message(1, node) + message(2, "OVMS_Studio_Demo") + message(11, value_info("input")) + message(12, value_info("output"))
    model = integer(1, 8) + message(2, "OVMS Studio") + message(7, graph) + message(8, integer(2, 13))
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(model)
    return destination


if __name__ == "__main__":
    root = Path(__file__).resolve().parent.parent
    path = create_model(root / "examples" / "tiny-add" / "1" / "model.onnx")
    print(f"Created {path} ({path.stat().st_size} bytes)")
