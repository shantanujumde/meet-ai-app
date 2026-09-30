/**
 * Onboarding step 3: how speech becomes text on this Mac.
 *
 * The card is Settings' own {@link EngineSummary}, so setup and Settings can
 * never describe the same machine differently.
 */

import { EngineSummary } from "@/ui/engine/EngineSummary";

export function SpeechStep({ onNext }: { onNext: () => void }) {
  return (
    <>
      <header className="page__header">
        <h1 className="page__title">How meet-ai turns speech into text</h1>
      </header>
      <p className="prose">
        This happens on your Mac, not on a server. Newer Macs have Apple's speech engine built in
        and need nothing at all; older ones use a model meet-ai downloads once and then keeps.
      </p>

      <EngineSummary />

      <div className="btn-row">
        <button type="button" className="btn btn--primary" onClick={onNext}>
          Continue
        </button>
      </div>
    </>
  );
}
