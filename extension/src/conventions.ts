/** The bot plays H-Group at levels 1 to 12, Reactor and Referential Sieve. */
export const CONVENTIONS: ReadonlyArray<readonly [string, string]> = [
  ...Array.from({ length: 12 }, (_, i): readonly [string, string] => [
    `HGroup${i + 1}`,
    `H-Group (level ${i + 1})`,
  ]),
  ["Reactor", "Reactor"],
  ["RefSieve", "Referential Sieve"],
];
