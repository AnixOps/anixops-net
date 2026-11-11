import markUrl from "./design/anixops/brand/mark.svg";

/** The AnixOps mark (slate tile, gradient orbit A) from AnixOps Design; never redrawn here. */
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
