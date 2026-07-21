#!/usr/bin/env python3
"""Strict, bounded PNG validation for hardware-gate framebuffer evidence."""

import binascii
import hashlib
import struct
import sys
import zlib

MAX_CAPTURE_PNG_BYTES = 64 * 1024 * 1024
MAX_CAPTURE_PIXELS = 32 * 1024 * 1024
MAX_DECODED_PNG_BYTES = 256 * 1024 * 1024


def validate_png_bytes(encoded, expected_dimensions=None):
    """Validate already-opened PNG bytes without reopening a filesystem path."""

    if len(encoded) > MAX_CAPTURE_PNG_BYTES:
        raise ValueError("PNG exceeds the fixed 64 MiB capture limit")
    if not encoded.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ValueError("file has no PNG signature")

    offset = 8
    ihdr = None
    palette = None
    compressed = bytearray()
    saw_idat = False
    idat_ended = False
    saw_iend = False
    while offset < len(encoded):
        if offset + 12 > len(encoded):
            raise ValueError("PNG has a truncated chunk header")
        length = struct.unpack(">I", encoded[offset : offset + 4])[0]
        chunk_type = encoded[offset + 4 : offset + 8]
        if len(chunk_type) != 4 or any(
            not (65 <= byte <= 90 or 97 <= byte <= 122) for byte in chunk_type
        ):
            raise ValueError("PNG has an invalid chunk type")
        if chunk_type[2] & 0x20:
            raise ValueError("PNG sets the reserved chunk-type bit")
        chunk_end = offset + 12 + length
        if chunk_end > len(encoded):
            raise ValueError("PNG has a truncated chunk payload")
        payload = encoded[offset + 8 : offset + 8 + length]
        recorded_crc = struct.unpack(
            ">I", encoded[offset + 8 + length : chunk_end]
        )[0]
        actual_crc = binascii.crc32(chunk_type)
        actual_crc = binascii.crc32(payload, actual_crc) & 0xFFFFFFFF
        if recorded_crc != actual_crc:
            raise ValueError("PNG has a chunk CRC mismatch")
        if saw_idat and chunk_type != b"IDAT":
            idat_ended = True

        if chunk_type == b"IHDR":
            if ihdr is not None or offset != 8 or length != 13:
                raise ValueError("PNG has an invalid IHDR")
            ihdr = struct.unpack(">IIBBBBB", payload)
        elif chunk_type == b"PLTE":
            if ihdr is None or palette is not None or saw_idat:
                raise ValueError("PNG has a misplaced or repeated PLTE")
            if length == 0 or length > 768 or length % 3 != 0:
                raise ValueError("PNG has an invalid PLTE")
            palette = payload
        elif chunk_type == b"IDAT":
            if ihdr is None or idat_ended or saw_iend:
                raise ValueError("PNG has non-consecutive or misplaced IDAT")
            saw_idat = True
            compressed.extend(payload)
        elif chunk_type == b"IEND":
            if ihdr is None or not saw_idat or length != 0 or chunk_end != len(encoded):
                raise ValueError("PNG has an invalid or non-final IEND")
            saw_iend = True
        elif chunk_type[0] & 0x20 == 0:
            raise ValueError(f"PNG has unknown critical chunk {chunk_type!r}")
        offset = chunk_end

    if ihdr is None or not compressed or not saw_iend:
        raise ValueError("PNG is missing required image chunks")
    width, height, bit_depth, color_type, compression, filter_method, interlace = ihdr
    if expected_dimensions is not None and (width, height) != expected_dimensions:
        raise ValueError(
            f"PNG dimensions {(width, height)} do not match {expected_dimensions}"
        )
    if width == 0 or height == 0 or width * height > MAX_CAPTURE_PIXELS:
        raise ValueError("PNG has invalid or excessive dimensions")
    if compression != 0 or filter_method != 0 or interlace != 0:
        raise ValueError("PNG uses an unsupported encoding contract")

    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}.get(color_type)
    valid_depths = {
        0: {1, 2, 4, 8, 16},
        2: {8, 16},
        3: {1, 2, 4, 8},
        4: {8, 16},
        6: {8, 16},
    }
    if channels is None or bit_depth not in valid_depths[color_type]:
        raise ValueError("PNG uses an invalid color/depth combination")
    if color_type == 3:
        if palette is None or len(palette) // 3 > 1 << bit_depth:
            raise ValueError("indexed PNG has no valid palette")
    elif color_type in (0, 4) and palette is not None:
        raise ValueError("grayscale PNG must not contain a palette")

    row_bytes = (width * channels * bit_depth + 7) // 8
    expected_raw_bytes = (row_bytes + 1) * height
    if expected_raw_bytes > MAX_DECODED_PNG_BYTES:
        raise ValueError("PNG exceeds the fixed decoded-byte limit")
    try:
        decompressor = zlib.decompressobj()
        raw = decompressor.decompress(bytes(compressed), expected_raw_bytes + 1)
    except zlib.error as error:
        raise ValueError(f"PNG pixel stream could not be decoded: {error}") from error
    if (
        len(raw) != expected_raw_bytes
        or not decompressor.eof
        or decompressor.unused_data
        or decompressor.unconsumed_tail
    ):
        raise ValueError("PNG has an incomplete, excessive, or trailing pixel stream")
    if any(raw[row * (row_bytes + 1)] > 4 for row in range(height)):
        raise ValueError("PNG contains an invalid scanline filter")
    return hashlib.sha256(encoded).hexdigest(), width, height


def validate_png(path, expected_dimensions=None):
    with open(path, "rb") as handle:
        encoded = handle.read(MAX_CAPTURE_PNG_BYTES + 1)
    return validate_png_bytes(encoded, expected_dimensions)


def main():
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <png-file>", file=sys.stderr)
        return 2
    try:
        png_sha256, width, height = validate_png(sys.argv[1])
    except (OSError, ValueError, struct.error) as error:
        print(f"invalid capture PNG: {error}", file=sys.stderr)
        return 1
    print(f"{png_sha256} {width} {height}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
