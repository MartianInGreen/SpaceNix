import * as React from "react";

/**
 * Returns a number that increments every `intervalMs` milliseconds. The
 * exact value is the current `Date.now()`; the important property is that
 * it *changes*, so any component using this hook re-renders on the
 * interval. Pair it with a relative-time computation against a fixed
 * timestamp to make "x seconds ago" labels tick.
 */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = React.useState(() => Date.now());
  React.useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}
