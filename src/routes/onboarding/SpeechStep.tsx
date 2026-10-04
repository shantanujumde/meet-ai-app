/**
 * Onboarding step 3: how speech becomes text on this Mac.
 *
 * The card is Settings' own {@link EngineSummary}, so setup and Settings can
 * never describe the same machine differently.
 */

import { ArrowRight, Captions } from "lucide-react";
import { EngineSummary } from "@/ui/engine/EngineSummary";
import { IconSquare } from "@/ui/icons";
import { Button, ButtonRow, Prose } from "@/ui/primitives";

export function SpeechStep({ onNext }: { onNext: () => void }) {
  return (
    <>
      <header className="page__header">
        <h1 className="page__title flex items-center gap-4">
          <IconSquare icon={Captions} />
          How meet-ai turns speech into text
        </h1>
      </header>
      <Prose>
        This happens on your Mac, not on a server. Newer Macs have Apple's speech engine built in
        and need nothing at all. Older ones use a model meet-ai downloads once and then keeps.
      </Prose>

      <EngineSummary />

      <ButtonRow>
        <Button tone="primary" icon={ArrowRight} onClick={onNext}>
          Continue
        </Button>
      </ButtonRow>
    </>
  );
}
