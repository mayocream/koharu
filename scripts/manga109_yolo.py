#!/usr/bin/env python3
import os
import argparse
import manga109api
import shutil
import random
import math


def convert_to_yolo_format(x_min, y_min, x_max, y_max, img_width, img_height):
    """Convert bounding box from Manga109 format to YOLO format."""
    x_center = ((x_min + x_max) / 2) / img_width
    y_center = ((y_min + y_max) / 2) / img_height
    width = (x_max - x_min) / img_width
    height = (y_max - y_min) / img_height

    return x_center, y_center, width, height


def process_annotation(ann, class_id, img_width, img_height, out_file):
    """Process a single annotation and write to output file."""
    x_min = int(ann["@xmin"])
    y_min = int(ann["@ymin"])
    x_max = int(ann["@xmax"])
    y_max = int(ann["@ymax"])

    x_center, y_center, width, height = convert_to_yolo_format(
        x_min, y_min, x_max, y_max, img_width, img_height
    )

    out_file.write(
        f"{class_id} {x_center:.6f} {y_center:.6f} {width:.6f} {height:.6f}\n"
    )


def manga109_to_yolo(manga109_root_dir, output_dir):
    """Convert Manga109 annotations to YOLO format with 80/20 train/val split."""
    # Initialize parser
    parser = manga109api.Parser(root_dir=manga109_root_dir)

    # Define class mapping
    class_map = {"frame": 0, "text": 1}

    # Create directory structure
    os.makedirs(os.path.join(output_dir, "images", "train"), exist_ok=True)
    os.makedirs(os.path.join(output_dir, "images", "val"), exist_ok=True)
    os.makedirs(os.path.join(output_dir, "labels", "train"), exist_ok=True)
    os.makedirs(os.path.join(output_dir, "labels", "val"), exist_ok=True)

    # Write class names file
    with open(os.path.join(output_dir, "classes.txt"), "w") as f:
        for class_name in ["frame", "text"]:
            f.write(f"{class_name}\n")

    book_list = parser.books

    # Shuffle books to ensure random distribution
    random.shuffle(book_list)

    # Calculate the split point (80% for training, 20% for validation)
    split_idx = math.ceil(len(book_list) * 0.8)
    train_books = book_list[:split_idx]
    val_books = book_list[split_idx:]

    print(f"Training books: {len(train_books)}")
    print(f"Validation books: {len(val_books)}")

    # Process training books
    process_books(parser, train_books, output_dir, class_map, "train")

    # Process validation books
    process_books(parser, val_books, output_dir, class_map, "val")

    # Create YAML configuration file for YOLO
    yaml_path = os.path.join(output_dir, "data.yaml")
    with open(yaml_path, "w") as f:
        f.write(f"path: {os.path.abspath(output_dir)}\n")
        f.write("train: images/train\n")
        f.write("val: images/val\n\n")

        f.write("names:\n")
        for i, name in enumerate(["frame", "text"]):
            f.write(f"  {i}: {name}\n")


def process_books(parser, book_list, output_dir, class_map, split_type):
    """Process books for either train or val split."""
    for book in book_list:
        print(f"Processing {book} for {split_type}...")

        # Get annotation data
        annotation = parser.get_annotation(book=book)

        # Process each page in the book
        for page in annotation["page"]:
            page_idx = page["@index"]
            img_width = int(page["@width"])
            img_height = int(page["@height"])

            # Create unique filename
            filename = f"{book}_{page_idx:03d}"

            # Copy the image
            img_src_path = parser.img_path(book=book, index=page_idx)
            img_dst_path = os.path.join(
                output_dir, "images", split_type, f"{filename}.jpg"
            )

            if os.path.exists(img_src_path):
                shutil.copy2(img_src_path, img_dst_path)

            # Create annotation file
            label_path = os.path.join(
                output_dir, "labels", split_type, f"{filename}.txt"
            )

            with open(label_path, "w") as f:
                # Process each annotation type
                for ann_type in ["frame", "text"]:
                    if ann_type in page:
                        # Handle both single annotation and list of annotations
                        annotations = page[ann_type]
                        if not isinstance(annotations, list):
                            annotations = [annotations]

                        for ann in annotations:
                            process_annotation(
                                ann, class_map[ann_type], img_width, img_height, f
                            )


def main():
    parser = argparse.ArgumentParser(
        description="Convert Manga109 dataset to YOLO format with 80/20 train/val split"
    )
    parser.add_argument(
        "--manga109_dir", required=True, help="Path to Manga109 dataset root directory"
    )
    parser.add_argument(
        "--output_dir",
        required=True,
        help="Output directory for YOLO-formatted dataset",
    )
    parser.add_argument(
        "--seed", type=int, default=42, help="Random seed for dataset splitting"
    )

    args = parser.parse_args()

    # Set random seed for reproducibility
    random.seed(args.seed)

    manga109_to_yolo(args.manga109_dir, args.output_dir)

    print(f"Conversion complete! Output saved to {args.output_dir}")
    print(f"Dataset split: 80% training, 20% validation")


if __name__ == "__main__":
    main()
