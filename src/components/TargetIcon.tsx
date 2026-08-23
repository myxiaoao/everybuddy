import codeBuddyIcon from "@/assets/codebuddy.png";
import workBuddyIcon from "@/assets/workbuddy.png";
import type { TargetKind } from "@/types";

const targetIcons: Record<TargetKind, string> = {
  workbuddy: workBuddyIcon,
  codebuddy: codeBuddyIcon,
};

export function TargetIcon({ target }: { target: TargetKind }) {
  return (
    <span
      className="target-option__icon"
      data-target-kind={target}
      aria-hidden="true"
    >
      <img src={targetIcons[target]} alt="" width={32} height={32} />
    </span>
  );
}
