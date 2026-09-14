import { useId, useState } from "react";
import type { PublishSourceConflict } from "../lib/publish-selection";
import type { createTranslator } from "../lib/i18n";
import { Button } from "./ui/button";
import { Modal } from "./Modal";

export function PublishSourcesDialog({
  conflicts,
  t,
  onClose,
  onConfirm,
}: {
  conflicts: PublishSourceConflict[];
  t: ReturnType<typeof createTranslator>;
  onClose: () => void;
  onConfirm: (choices: Record<string, string>) => void;
}) {
  const groupId = useId();
  const [choices, setChoices] = useState<Record<string, string>>({});
  const complete = conflicts.every((conflict) =>
    conflict.candidates.some(
      (candidate) => candidate.id === choices[conflict.modelId],
    ),
  );
  return (
    <Modal
      open
      title={t("choosePublishSources")}
      description={t("publishSourceConflictHint", { count: conflicts.length })}
      closeLabel={t("close")}
      onClose={onClose}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            {t("cancel")}
          </Button>
          <Button disabled={!complete} onClick={() => onConfirm(choices)}>
            {t("continuePublishPreview")}
          </Button>
        </>
      }
    >
      <div className="publish-source-choices">
        {conflicts.map((conflict, index) => (
          <fieldset key={conflict.modelId}>
            <legend>
              <code>{conflict.modelId}</code>
            </legend>
            {conflict.candidates.map((candidate) => (
              <label key={candidate.id}>
                <input
                  type="radio"
                  name={`${groupId}-${index}`}
                  value={candidate.id}
                  checked={choices[conflict.modelId] === candidate.id}
                  onChange={() =>
                    setChoices((current) => ({
                      ...current,
                      [conflict.modelId]: candidate.id,
                    }))
                  }
                />
                <span>
                  <strong>{candidate.name}</strong>
                  <code>{candidate.apiRoot}</code>
                </span>
              </label>
            ))}
          </fieldset>
        ))}
      </div>
    </Modal>
  );
}
