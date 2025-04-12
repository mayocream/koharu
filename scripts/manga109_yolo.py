#!/usr/bin/env python3
import os
import argparse
import manga109api
import shutil

def convert_to_yolo_format(x_min, y_min, x_max, y_max, img_width, img_height):
    """Convert bounding box from Manga109 format to YOLO format."""
    x_center = ((x_min + x_max) / 2) / img_width
    y_center = ((y_min + y_max) / 2) / img_height
    width = (x_max - x_min) / img_width
    height = (y_max - y_min) / img_height

    return x_center, y_center, width, height

def process_annotation(ann, class_id, img_width, img_height, out_file):
    """Process a single annotation and write to output file."""
    x_min = int(ann['@xmin'])
    y_min = int(ann['@ymin'])
    x_max = int(ann['@xmax'])
    y_max = int(ann['@ymax'])

    x_center, y_center, width, height = convert_to_yolo_format(
        x_min, y_min, x_max, y_max, img_width, img_height
    )

    out_file.write(f"{class_id} {x_center:.6f} {y_center:.6f} {width:.6f} {height:.6f}\n")

def manga109_to_yolo(manga109_root_dir, output_dir, selected_books=None):
    """Convert Manga109 annotations to YOLO format."""
    # Initialize parser
    parser = manga109api.Parser(root_dir=manga109_root_dir)

    # Get list of books
    books = selected_books if selected_books else parser.books

    # Define class mapping
    class_map = {
        'text': 0,
        'face': 1,
        'body': 2,
        'frame': 3
    }

    # Create directory structure
    os.makedirs(os.path.join(output_dir, 'images'), exist_ok=True)
    os.makedirs(os.path.join(output_dir, 'labels'), exist_ok=True)

    # Write class names file
    with open(os.path.join(output_dir, 'classes.txt'), 'w') as f:
        for class_name in ['text', 'face', 'body', 'frame']:
            f.write(f"{class_name}\n")

    # Process each book
    for book in books:
        print(f"Processing {book}...")

        # Get annotation data
        annotation = parser.get_annotation(book=book)

        # Process each page in the book
        for page in annotation["page"]:
            page_idx = page["@index"]
            img_width = page["@width"]
            img_height = page["@height"]

            # Create unique filename
            filename = f"{book}_{page_idx:03d}"

            # Copy the image
            img_src_path = parser.img_path(book=book, index=page_idx)
            img_dst_path = os.path.join(output_dir, 'images', f"{filename}.jpg")

            if os.path.exists(img_src_path):
                shutil.copy2(img_src_path, img_dst_path)

            # Create annotation file
            label_path = os.path.join(output_dir, 'labels', f"{filename}.txt")

            with open(label_path, 'w') as f:
                # Process each annotation type
                for ann_type in ['text', 'face', 'body', 'frame']:
                    if ann_type in page:
                        for ann in page[ann_type]:
                            process_annotation(ann, class_map[ann_type], img_width, img_height, f)

    # Create YAML configuration file for YOLO
    yaml_path = os.path.join(output_dir, 'manga109.yaml')
    with open(yaml_path, 'w') as f:
        f.write(f"path: {os.path.abspath(output_dir)}\n")
        f.write("train: images\n")
        f.write("val: images\n\n")

        f.write("names:\n")
        for i, name in enumerate(['text', 'face', 'body', 'frame']):
            f.write(f"  {i}: {name}\n")

def main():
    parser = argparse.ArgumentParser(description='Convert Manga109 dataset to YOLO format')
    parser.add_argument('--manga109_dir', required=True, help='Path to Manga109 dataset root directory')
    parser.add_argument('--output_dir', required=True, help='Output directory for YOLO-formatted dataset')
    parser.add_argument('--books', nargs='*', help='Specific books to convert (default: all books)')

    args = parser.parse_args()

    manga109_to_yolo(args.manga109_dir, args.output_dir, args.books)

    print(f"Conversion complete! Output saved to {args.output_dir}")

if __name__ == "__main__":
    main()
