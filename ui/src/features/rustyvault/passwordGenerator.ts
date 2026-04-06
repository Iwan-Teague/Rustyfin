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
const MEMORABLE_WORDS = [
  'amber',
  'anchor',
  'cabin',
  'cactus',
  'cedar',
  'cipher',
  'drift',
  'ember',
  'forest',
  'garden',
  'harvest',
  'juniper',
  'market',
  'meadow',
  'mint',
  'paper',
  'pepper',
  'raven',
  'river',
  'sunrise',
  'thunder',
  'velvet',
  'whisper',
  'zephyr',
];
const MEMORABLE_NUMBER_SWAP = [
  ['a', '4'],
  ['e', '3'],
  ['i', '1'],
  ['o', '0'],
  ['s', '5'],
  ['t', '7'],
] as const;
const MEMORABLE_SYMBOL_SWAP = [
  ['a', '@'],
  ['s', '$'],
  ['x', '*'],
] as const;
const MEMORABLE_SEPARATORS = ['-', '.', '_', '!'];

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

function randomChoice<T>(items: readonly T[]): T {
  return items[getRandomInt(items.length)];
}

function applyWordCase(word: string, options: PasswordGeneratorOptions) {
  if (options.include_uppercase && options.include_lowercase) {
    return word.charAt(0).toUpperCase() + word.slice(1);
  }
  if (options.include_uppercase) {
    return word.toUpperCase();
  }
  if (options.include_lowercase) {
    return word.toLowerCase();
  }
  throw new Error('Memorable passwords require uppercase or lowercase letters');
}

function canUseMemorableSubstitution(
  replacement: string,
  options: PasswordGeneratorOptions,
  kind: 'number' | 'symbol',
) {
  if (kind === 'number' && !options.include_numbers) return false;
  if (kind === 'symbol' && !options.include_symbols) return false;
  if (options.exclude_ambiguous && [...replacement].some((char) => AMBIGUOUS.has(char))) {
    return false;
  }
  return true;
}

function replaceFirstMatch(
  source: string,
  replacements: readonly (readonly [string, string])[],
  options: PasswordGeneratorOptions,
  kind: 'number' | 'symbol',
) {
  for (const [needle, replacement] of replacements) {
    if (!canUseMemorableSubstitution(replacement, options, kind)) {
      continue;
    }
    const idx = source.toLowerCase().indexOf(needle);
    if (idx >= 0) {
      return source.slice(0, idx) + replacement + source.slice(idx + 1);
    }
  }
  return source;
}

function nextAllowedChar(options: PasswordGeneratorOptions, kindPreference?: 'number' | 'symbol') {
  const pools: string[] = [];
  if (kindPreference !== 'symbol' && options.include_numbers) {
    pools.push(filterAmbiguous(NUMBERS, options.exclude_ambiguous));
  }
  if (kindPreference !== 'number' && options.include_symbols) {
    pools.push(filterAmbiguous(SYMBOLS, options.exclude_ambiguous));
  }
  if (options.include_uppercase) pools.push(filterAmbiguous(UPPERCASE, options.exclude_ambiguous));
  if (options.include_lowercase) pools.push(filterAmbiguous(LOWERCASE, options.exclude_ambiguous));
  const combined = pools.join('');
  if (!combined) {
    throw new Error('Select at least one password character group');
  }
  return combined[getRandomInt(combined.length)];
}

function buildMemorableCandidate(options: PasswordGeneratorOptions): string {
  if (!options.include_uppercase && !options.include_lowercase) {
    throw new Error('Memorable passwords require uppercase or lowercase letters');
  }

  const filteredWords = options.exclude_ambiguous
    ? MEMORABLE_WORDS.filter((word) => ![...word].some((char) => AMBIGUOUS.has(char)))
    : MEMORABLE_WORDS;
  const usableWords = filteredWords.length > 0 ? filteredWords : MEMORABLE_WORDS;
  const targetLength = Math.max(12, options.length);
  const words: string[] = [];
  let rawLength = 0;

  while (rawLength < Math.max(8, targetLength - 3)) {
    const word = randomChoice(usableWords);
    words.push(word);
    rawLength += word.length;
    if (words.length >= 4) {
      break;
    }
  }
  if (words.length === 0) {
    words.push(randomChoice(usableWords));
  }

  let candidate = words.map((word) => applyWordCase(word, options)).join('');
  const beforeNumbers = candidate;
  candidate = replaceFirstMatch(candidate, MEMORABLE_NUMBER_SWAP, options, 'number');
  const beforeSymbols = candidate;
  candidate = replaceFirstMatch(candidate, MEMORABLE_SYMBOL_SWAP, options, 'symbol');

  if (options.include_symbols && candidate === beforeSymbols && candidate.length < targetLength) {
    const separator = filterAmbiguous(randomChoice(MEMORABLE_SEPARATORS), options.exclude_ambiguous);
    if (separator) {
      const insertAt = Math.max(1, Math.floor(candidate.length / 2));
      candidate = candidate.slice(0, insertAt) + separator + candidate.slice(insertAt);
    }
  }

  if (options.include_numbers && candidate === beforeNumbers) {
    candidate += nextAllowedChar(options, 'number');
  }

  if (candidate.length > targetLength) {
    candidate = candidate.slice(0, targetLength);
  }
  while (candidate.length < targetLength) {
    candidate += nextAllowedChar(options);
  }
  return candidate;
}

export function generatePassword(
  options: PasswordGeneratorOptions,
  preset: PasswordGeneratorPreset = 'balanced',
): string {
  if (preset === 'memorable') {
    return buildMemorableCandidate(options);
  }
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
