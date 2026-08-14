export function DevicePreview() {
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute right-3 top-3 z-10 hidden size-28 select-none md:block lg:right-5 lg:top-5 lg:size-36"
    >
      <div className="absolute inset-[12%] rounded-full bg-primary/20 blur-2xl dark:bg-primary/30" />
      <div className="absolute inset-x-[2%] top-[29%] h-[51%] [transform:rotate(-11deg)_skewX(-5deg)]">
        <div className="absolute inset-x-[2%] top-[13%] h-full translate-y-[10%] rounded-[24%] bg-zinc-950/90 shadow-[0_24px_28px_-16px_rgb(0_0_0/0.8)]" />
        <div className="relative h-full overflow-hidden rounded-[24%] border border-white/15 bg-[linear-gradient(145deg,#303532_0%,#151816_52%,#080a09_100%)] shadow-[inset_0_1px_0_rgb(255_255_255/0.2),0_14px_28px_-12px_rgb(0_0_0/0.85)]">
          <div className="absolute inset-x-[8%] top-[8%] h-px bg-gradient-to-r from-transparent via-white/35 to-transparent" />
          <div className="absolute inset-y-[17%] left-[10%] right-[10%] overflow-hidden rounded-[20%] border border-emerald-200/20 bg-[linear-gradient(135deg,#0b8f6c_0%,#08694f_62%,#064a39_100%)] shadow-[inset_0_1px_0_rgb(255_255_255/0.2)]">
            <div className="absolute -right-[15%] -top-[45%] size-[90%] rounded-full bg-white/10 blur-md" />
            <div className="absolute left-[14%] top-1/2 size-[32%] -translate-y-1/2 rounded-full border-[2px] border-emerald-50/90 shadow-[0_0_12px_rgb(167_243_208/0.22)]">
              <div className="absolute inset-[27%] rounded-full bg-emerald-50/95" />
            </div>
            <div className="absolute right-[13%] top-[34%] h-[9%] w-[31%] rounded-full bg-emerald-50/90" />
            <div className="absolute right-[18%] top-[55%] h-[7%] w-[26%] rounded-full bg-emerald-100/55" />
          </div>
          <div className="absolute bottom-[7%] left-[15%] flex gap-[4px] lg:gap-[5px]">
            <span className="size-[3px] rounded-full bg-emerald-300/85 shadow-[0_0_6px_rgb(110_231_183/0.7)] lg:size-1" />
            <span className="size-[3px] rounded-full bg-white/25 lg:size-1" />
          </div>
          <div className="absolute bottom-[8%] right-[15%] h-[5%] w-[14%] rounded-full bg-black/70 shadow-[inset_0_1px_1px_rgb(255_255_255/0.12)]" />
        </div>
      </div>
    </div>
  );
}
