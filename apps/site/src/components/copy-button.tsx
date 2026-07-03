"use client";

import { useState } from "react";
import { CheckIcon, CopyIcon } from "lucide-react";
import { Button } from "@/components/ui/button";

type CopyButtonProps = {
  value: string;
  label: string;
};

export function CopyButton({ value, label }: CopyButtonProps) {
  const [status, setStatus] = useState<"idle" | "copied" | "blocked">("idle");

  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      setStatus("copied");
    } catch {
      setStatus("blocked");
    }

    window.setTimeout(() => setStatus("idle"), 1800);
  }

  const copied = status === "copied";
  const blocked = status === "blocked";

  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      onClick={copy}
      aria-label={label}
      aria-live="polite"
    >
      {copied ? (
        <CheckIcon data-icon="inline-start" />
      ) : (
        <CopyIcon data-icon="inline-start" />
      )}
      {copied ? "Copied" : blocked ? "Copy blocked" : "Copy"}
    </Button>
  );
}
