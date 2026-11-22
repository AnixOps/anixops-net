import markUrl from "./design/anixops/brand/products/net/icon.svg";

/** The AnixOps Net product icon (AnixOps Design brand.md 2.6); never redrawn here. */
export function BrandMark({ size = 22 }: { size?: number }) {
  return <img src={markUrl} width={size} height={size} alt="" aria-hidden="true" />;
}

/** The AnixOps Design lockup: the mark, "AnixOps" in semibold and the product word in regular label-2. */
export function Wordmark() {
  return (
    <span className="wordmark">
      <BrandMark />
      <span>AnixOps <em>Net</em></span>
    </span>
  );
}
