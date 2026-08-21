"use client";

import Image from "next/image";
import { ArrowDownIcon, ArrowUpRightIcon, MenuIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";

const navigation = [
  ["01", "Build Studio", "#experience"],
  ["02", "Desktop", "#desktop"],
  ["03", "Control", "#system"],
  ["04", "Get the preview", "#download"],
  ["05", "Verify", "#verify"],
] as const;

export function MobileNavigation() {
  return (
    <Sheet>
      <SheetTrigger asChild>
        <Button
          className="site-header__menu"
          type="button"
          variant="outline"
          size="icon"
          aria-label="Open navigation"
        >
          <MenuIcon aria-hidden="true" />
        </Button>
      </SheetTrigger>
      <SheetContent className="mobile-nav">
        <div className="mobile-nav__header">
          <Image src="/favicon.svg" alt="" width={28} height={28} aria-hidden="true" />
          <SheetTitle>Explore Goblins OS</SheetTitle>
          <SheetDescription>
            Meet Build Studio, see the desktop, and get the public preview.
          </SheetDescription>
        </div>
        <nav className="mobile-nav__links" aria-label="Mobile navigation">
          {navigation.map(([index, label, href]) => (
            <SheetClose asChild key={href}>
              <a href={href}>
                <span>{index}</span>
                <strong>{label}</strong>
                <ArrowDownIcon aria-hidden="true" />
              </a>
            </SheetClose>
          ))}
        </nav>
        <SheetClose asChild>
          <a
            className="mobile-nav__source"
            href="https://github.com/Joe-Simo/goblins-os"
            target="_blank"
            rel="noreferrer"
          >
            Explore the source
            <ArrowUpRightIcon aria-hidden="true" />
          </a>
        </SheetClose>
      </SheetContent>
    </Sheet>
  );
}
