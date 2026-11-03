export interface Store<S> {
  get: () => S;
  update: (change: Partial<S> | ((state: S) => Partial<S>)) => void;
  subscribe: (listener: (state: S) => void) => void;
}

/** A state object that tells its listeners when a part of it has changed. */
export function createStore<S extends object>(initial: S): Store<S> {
  let state = initial;
  const listeners: Array<(state: S) => void> = [];

  return {
    get: () => state,
    update(change) {
      const next = typeof change === "function" ? change(state) : change;
      const keys = Object.keys(next) as Array<keyof S>;
      if (keys.every((key) => Object.is(state[key], next[key]))) {
        return;
      }

      state = { ...state, ...next };
      for (const listener of listeners) {
        listener(state);
      }
    },
    subscribe(listener) {
      listeners.push(listener);
    },
  };
}
