/** Onboarding step 1: what meet-ai is, and what it will never do. */

import { ArrowRight, Sparkles } from "lucide-react";
import { IconSquare } from "@/ui/icons";
import { Button, ButtonRow, Prose } from "@/ui/primitives";

export function WelcomeStep({ onNext }: { onNext: () => void }) {
  return (
    <>
      <header className="page__header">
        <h1 className="page__title flex items-center gap-4">
          <IconSquare icon={Sparkles} />
          meet-ai records your meetings
        </h1>
      </header>
      <Prose>
        It listens to your microphone and to whatever your Mac is playing, writes both sides out as
        plain markdown, and stops. There is no bot in your call and no account to make.
      </Prose>
      <Prose>
        Nothing is uploaded. Recordings, transcripts and notes stay in a folder on this Mac, in
        files you can open in any editor. The only thing meet-ai ever downloads is a speech model,
        and only if this Mac needs one.
      </Prose>
      <Prose>Four things to set up, then you are done.</Prose>
      <ButtonRow>
        <Button tone="primary" icon={ArrowRight} onClick={onNext}>
          Get started
        </Button>
      </ButtonRow>
    </>
  );
}
