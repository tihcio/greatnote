import { useEffect, useState } from "react";
import { t } from "../i18n";
import { subscribePrompt } from "../lib/prompt";
import type { PromptRequest } from "../lib/prompt";
import PlannerDialog from "./PlannerDialog";

/** Dialog per `askText`, montato una volta nell'app. */
export default function PromptHost() {
  const [request, setRequest] = useState<PromptRequest | null>(null);
  const [value, setValue] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(
    () =>
      subscribePrompt((next) => {
        setRequest(next);
        setValue(next?.initialValue ?? "");
        setError(null);
      }),
    [],
  );

  if (!request) return null;
  const submit = () => {
    const trimmed = value.trim();
    const problem = request.validate?.(trimmed) ?? (trimmed ? null : t.valueRequired);
    if (problem) {
      setError(problem);
      return;
    }
    request.resolve(trimmed);
  };

  return (
    <PlannerDialog mark="✎" onClose={() => request.resolve(null)} title={request.title}>
      <form
        className="organization-dialog-form"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label className="dialog-field">
          <span>{request.label}</span>
          <input
            maxLength={request.maxLength ?? 200}
            onChange={(event) => {
              setValue(event.target.value);
              setError(null);
            }}
            placeholder={request.placeholder}
            value={value}
          />
        </label>
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        <footer className="organization-dialog-actions">
          <button className="dialog-cancel-button" onClick={() => request.resolve(null)} type="button">
            {t.cancel}
          </button>
          <button className="dialog-submit-button" type="submit">
            {request.confirmLabel}
          </button>
        </footer>
      </form>
    </PlannerDialog>
  );
}
