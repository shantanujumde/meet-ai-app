/** Onboarding step 1: what meet-ai is, and what it will never do. */

export function WelcomeStep({ onNext }: { onNext: () => void }) {
  return (
    <>
      <header className="page__header">
        <h1 className="page__title">meet-ai records your meetings</h1>
      </header>
      <p className="prose">
        It listens to your microphone and to whatever your Mac is playing, writes both sides out as
        plain markdown, and stops. There is no bot in your call and no account to make.
      </p>
      <p className="prose">
        Nothing is uploaded. Recordings, transcripts and notes stay in a folder on this Mac, in
        files you can open in any editor. The only thing meet-ai ever downloads is a speech model,
        and only if this Mac needs one.
      </p>
      <p className="prose">Two things to set up, then you are done.</p>
      <div className="btn-row">
        <button type="button" className="btn btn--primary" onClick={onNext}>
          Get started
        </button>
      </div>
    </>
  );
}
