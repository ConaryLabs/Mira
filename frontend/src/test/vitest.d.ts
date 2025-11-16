// src/test/vitest.d.ts
// Type declarations for @testing-library/jest-dom matchers in Vitest

import '@testing-library/jest-dom'
import { TestingLibraryMatchers } from '@testing-library/jest-dom/matchers'

declare module 'vitest' {
  interface Assertion<T = any> extends TestingLibraryMatchers<T, void> {}
  interface AsymmetricMatchersContaining extends TestingLibraryMatchers {}
}
