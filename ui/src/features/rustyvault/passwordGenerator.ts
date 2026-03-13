'use client';

export type PasswordGeneratorPreset = 'memorable' | 'balanced' | 'maximum';

export type PasswordGeneratorOptions = {
  length: number;
  include_uppercase: boolean;
  include_lowercase: boolean;
  include_numbers: boolean;
  include_symbols: boolean;
  exclude_ambiguous: boolean;
};

const AMBIGUOUS = new Set(['0', 'O', 'o', '1', 'l', 'I']);
const UPPERCASE = 'ABCDEFGHJKLMNPQRSTUVWXYZ';
const LOWERCASE = 'abcdefghijkmnopqrstuvwxyz';
const NUMBERS = '23456789';
const SYMBOLS = '!@#$%^&*()-_=+[]{}:,.?';

export function presetOptions(preset: PasswordGeneratorPreset): PasswordGeneratorOptions {
  switch (preset) {
    case 'memorable':
      return {
        length: 18,
        include_uppercase: true,
        include_lowercase: true,
        include_numbers: true,
        include_symbols: false,
        exclude_ambiguous: true,
      };
    case 'maximum':
      return {
        length: 30,
        include_uppercase: true,
        include_lowercase: true,
        include_numbers: true,
        include_symbols: true,
        exclude_ambiguous: false,
      };
    case 'balanced':
    default:
      return {
        length: 22,
        include_uppercase: true,
        include_lowercase: true,
        include_numbers: true,
        include_symbols: true,
        exclude_ambiguous: true,
      };
  }
}

function filterAmbiguous(value: string, excludeAmbiguous: boolean) {
  if (!excludeAmbiguous) return value;
  return [...value].filter((char) => !AMBIGUOUS.has(char)).join('');
}

function getRandomInt(maxExclusive: number): number {
  const buf = new Uint32Array(1);
  crypto.getRandomValues(buf);
  return buf[0] % maxExclusive;
}

export function generatePassword(options: PasswordGeneratorOptions): string {
  const pools: string[] = [];
  if (options.include_uppercase) pools.push(filterAmbiguous(UPPERCASE, options.exclude_ambiguous));
  if (options.include_lowercase) pools.push(filterAmbiguous(LOWERCASE, options.exclude_ambiguous));
  if (options.include_numbers) pools.push(filterAmbiguous(NUMBERS, options.exclude_ambiguous));
  if (options.include_symbols) pools.push(SYMBOLS);

  if (pools.length === 0) {
    throw new Error('Select at least one password character group');
  }
  if (options.length < pools.length) {
    throw new Error('Password length is too short for the selected character groups');
  }

  const chars: string[] = [];
  const combined = pools.join('');

  for (const pool of pools) {
    chars.push(pool[getRandomInt(pool.length)]);
  }
  while (chars.length < options.length) {
    chars.push(combined[getRandomInt(combined.length)]);
  }

  for (let idx = chars.length - 1; idx > 0; idx -= 1) {
    const swapIdx = getRandomInt(idx + 1);
    [chars[idx], chars[swapIdx]] = [chars[swapIdx], chars[idx]];
  }

  return chars.join('');
}
