import re
import jaconv
import numpy as np
import time

from onnxruntime import InferenceSession
from PIL import Image


class MangaOCR:
    def __init__(self, encoder_model_path: str, decoder_model_path: str, vocab_path: str):
        self.encoder_session = InferenceSession(encoder_model_path)
        self.decoder_session = InferenceSession(decoder_model_path)
        self.vocab = self._load_vocab(vocab_path)

    def __call__(self, image: Image.Image) -> str:
        image = self._preprocess(image)

        # count time
        start = time.time()
        token_ids = self._generate(image)
        end = time.time()
        print(f"Time taken: {end - start:.2f} seconds")

        text = self._decode(token_ids)
        text = self._postprocess(text)

        return text

    def _load_vocab(self, vocab_file: str) -> list[str]:
        with open(vocab_file, "r", encoding="utf8") as f:
            vocab = f.read().splitlines()

        return vocab

    def _preprocess(self, image: Image.Image) -> np.ndarray:
        # convert to grayscale
        image = image.convert("L").convert("RGB")
        # resize
        image = image.resize((224, 224), resample=2)
        # rescale
        image = np.array(image, dtype=np.float32)
        image /= 255
        # normalize
        image = (image - 0.5) / 0.5
        # reshape from (224, 224, 3) to (3, 224, 224)
        image = image.transpose((2, 0, 1))
        # add batch size
        image = image[None]

        return image

    def _generate(self, image: np.ndarray) -> np.ndarray:
        encoder_hidden_states = self.encoder_session.run(None, {
            "pixel_values": image,
        })[0]

        token_ids = [2]

        for _ in range(300):
            [logits] = self.decoder_session.run(
                None,
                {
                    "encoder_hidden_states": encoder_hidden_states,
                    "input_ids": np.array([token_ids], dtype=np.int64),
                },
            )

            token_id = logits[0, -1, :].argmax()
            token_ids.append(int(token_id))

            if token_id == 3:
                break

        return token_ids

    def _decode(self, token_ids: list[int]) -> str:
        text = ""

        for token_id in token_ids:
            if token_id < 5:
                continue

            text += self.vocab[token_id]

        return text

    def _postprocess(self, text: str) -> str:
        text = "".join(text.split())
        text = text.replace("…", "...")
        text = re.sub("[・.]{2,}", lambda x: (x.end() - x.start()) * ".", text)
        text = jaconv.h2z(text, ascii=True, digit=True)

        return text


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description="Manga OCR with ONNX Runtime")
    parser.add_argument("--image", type=str, help="Path to the input image")
    parser.add_argument("--encoder-model", type=str, help="Path to the ONNX model file")
    parser.add_argument("--decoder-model", type=str, help="Path to the ONNX model file")
    parser.add_argument("--vocab", type=str, help="Path to the vocabulary file")
    args = parser.parse_args()

    ocr = MangaOCR(args.encoder_model, args.decoder_model, args.vocab)
    image = Image.open(args.image)
    text = ocr(image)
    print(text)
