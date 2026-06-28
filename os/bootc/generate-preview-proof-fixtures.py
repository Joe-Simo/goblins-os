#!/usr/bin/env python3
"""Generate installed Preview proof fixtures for the display-backed gate."""

from pathlib import Path
import base64


OUT = Path("/usr/share/goblins-os/proof")


def pdf_bytes() -> bytes:
    text = b"BT /F1 18 Tf 72 150 Td (Goblins OS Preview proof) Tj ET"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 320 220] "
        b"/Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        b"<< /Length "
        + str(len(text)).encode("ascii")
        + b" >>\nstream\n"
        + text
        + b"\nendstream",
    ]
    pdf = b"%PDF-1.4\n"
    offsets: list[int] = []
    for index, obj in enumerate(objects, start=1):
        offsets.append(len(pdf))
        pdf += f"{index} 0 obj\n".encode("ascii") + obj + b"\nendobj\n"
    xref = len(pdf)
    pdf += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode("ascii")
    for offset in offsets:
        pdf += f"{offset:010d} 00000 n \n".encode("ascii")
    pdf += (
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref}\n%%EOF\n"
    ).encode("ascii")
    return pdf


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "preview-open-render.pdf").write_bytes(pdf_bytes())
    (OUT / "preview-open-render.png").write_bytes(
        base64.b64decode(
            "iVBORw0KGgoAAAANSUhEUgAAAEAAAAAwCAIAAAAtV0A8AAAAhUlEQVR4nO3YQQqAIBBA0U3R"
            "/jdd5QJtRYywD/vDh7yY7AxzPSr1ps+47NbiJQDMiSi0ABQawC5TKmVqOV9rVgB7+8fN8tQK"
            "cGgBOBSAQwE4FIBDATgUgEMBOBSAQwE4FIBDATgUgEMBOBSAQwE4FIBDATgUgEMBOBSAQwE4"
            "FICzKbUAMlxmHNBZAAAAAElFTkSuQmCC"
        )
    )
    (OUT / "preview-open-render.txt").write_text(
        "Goblins OS Preview unsupported proof\n", encoding="utf-8"
    )


if __name__ == "__main__":
    main()
