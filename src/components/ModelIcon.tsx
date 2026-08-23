import type { CSSProperties } from "react";
import { resolveModelIcon } from "@/lib/model-icon";

interface ModelIdentity {
  id: string;
  name: string;
  vendor: string;
}

export function ModelIcon({ model }: { model: ModelIdentity }) {
  const match = resolveModelIcon(model);

  if (!match) {
    const fallback = Array.from(model.vendor.trim() || model.name.trim() || "?")
      .slice(0, 2)
      .join("")
      .toLocaleUpperCase();

    return (
      <span
        className="vendor-mark"
        data-model-brand="custom"
        aria-hidden="true"
      >
        {fallback}
      </span>
    );
  }

  const style = {
    "--model-icon-url": `url("${match.icon}")`,
  } as CSSProperties;

  return (
    <span
      className="vendor-mark"
      data-model-brand={match.brand}
      aria-hidden="true"
    >
      {match.colored ? (
        <img className="vendor-mark__image" src={match.icon} alt="" />
      ) : (
        <span className="vendor-mark__glyph" style={style} />
      )}
    </span>
  );
}
