import { useCallback, useRef } from "react";

export function useDebouncedSave(delay: number, save: (value: string) => void) {
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const flushRef = useRef<(() => void) | null>(null);

  const schedule = useCallback(
    (value: string) => {
      if (timer.current) clearTimeout(timer.current);
      flushRef.current = () => save(value);
      timer.current = setTimeout(() => {
        timer.current = null;
        flushRef.current = null;
        save(value);
      }, delay);
    },
    [delay, save],
  );

  const flush = useCallback(() => {
    if (timer.current) {
      clearTimeout(timer.current);
      timer.current = null;
    }
    flushRef.current?.();
    flushRef.current = null;
  }, []);

  return { schedule, flush };
}
