/** The empty chat (docs/plan/15 §8 "Welcome"); the composer and suggestions arrive with M1. */
export function EmptyChat() {
  return (
    <div className="flex h-full flex-col items-center justify-center px-6">
      <div className="w-full max-w-(--measure) text-center">
        <h1 className="text-hero font-semibold tracking-[-0.01em] text-fg">
          What should we work on?
        </h1>
        <p className="mt-2 text-body text-fg-2">
          Add a provider key in Settings to start a conversation.
        </p>
      </div>
    </div>
  );
}
