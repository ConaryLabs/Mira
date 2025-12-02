// src/components/__tests__/CodeBlock.test.tsx
// CodeBlock Component Tests

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { CodeBlock } from '../CodeBlock';

// Mock clipboard API
const mockWriteText = vi.fn();
Object.assign(navigator, {
  clipboard: {
    writeText: mockWriteText,
  },
});

describe('CodeBlock', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockWriteText.mockResolvedValue(undefined);
  });

  describe('rendering', () => {
    it('renders code content', () => {
      render(
        <CodeBlock
          code="console.log('hello');"
          language="javascript"
          isDark={true}
        />
      );

      expect(screen.getByText("console.log('hello');")).toBeInTheDocument();
    });

    it('displays the language label', () => {
      render(
        <CodeBlock
          code="const x = 1;"
          language="typescript"
          isDark={true}
        />
      );

      expect(screen.getByText('typescript')).toBeInTheDocument();
    });

    it('displays "text" when no language is provided', () => {
      render(
        <CodeBlock
          code="plain text"
          isDark={true}
        />
      );

      expect(screen.getByText('text')).toBeInTheDocument();
    });

    it('renders copy button', () => {
      render(
        <CodeBlock
          code="test code"
          language="rust"
          isDark={true}
        />
      );

      expect(screen.getByTitle('Copy code')).toBeInTheDocument();
      expect(screen.getByText('Copy')).toBeInTheDocument();
    });
  });

  describe('dark mode styling', () => {
    it('applies dark mode styles when isDark is true', () => {
      const { container } = render(
        <CodeBlock
          code="test"
          language="js"
          isDark={true}
        />
      );

      const outerDiv = container.firstChild as HTMLElement;
      expect(outerDiv.className).toContain('bg-gray-950');
      expect(outerDiv.className).toContain('border-gray-700');
    });

    it('applies light mode styles when isDark is false', () => {
      const { container } = render(
        <CodeBlock
          code="test"
          language="js"
          isDark={false}
        />
      );

      const outerDiv = container.firstChild as HTMLElement;
      expect(outerDiv.className).toContain('bg-gray-50');
      expect(outerDiv.className).toContain('border-gray-200');
    });
  });

  describe('copy functionality', () => {
    it('copies code to clipboard when copy button is clicked', async () => {
      const code = 'function test() { return true; }';
      render(
        <CodeBlock
          code={code}
          language="javascript"
          isDark={true}
        />
      );

      const copyButton = screen.getByTitle('Copy code');
      fireEvent.click(copyButton);

      expect(mockWriteText).toHaveBeenCalledWith(code);
    });

    it('shows "Copied" state after successful copy', async () => {
      render(
        <CodeBlock
          code="test code"
          language="rust"
          isDark={true}
        />
      );

      const copyButton = screen.getByTitle('Copy code');
      fireEvent.click(copyButton);

      await waitFor(() => {
        expect(screen.getByText('Copied')).toBeInTheDocument();
      });
    });

    it('reverts back to "Copy" after timeout', async () => {
      vi.useFakeTimers();

      render(
        <CodeBlock
          code="test code"
          language="rust"
          isDark={true}
        />
      );

      const copyButton = screen.getByTitle('Copy code');

      // Click and wait for the Promise to resolve
      await vi.waitFor(async () => {
        fireEvent.click(copyButton);
        await vi.advanceTimersByTimeAsync(100); // Let the async clipboard call resolve
      });

      expect(screen.getByText('Copied')).toBeInTheDocument();

      // Advance past the 2000ms timeout
      await vi.advanceTimersByTimeAsync(2100);

      expect(screen.getByText('Copy')).toBeInTheDocument();

      vi.useRealTimers();
    });

    it('handles clipboard write failure gracefully', async () => {
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      mockWriteText.mockRejectedValue(new Error('Clipboard error'));

      render(
        <CodeBlock
          code="test code"
          language="rust"
          isDark={true}
        />
      );

      const copyButton = screen.getByTitle('Copy code');
      fireEvent.click(copyButton);

      // Wait for the rejected promise to be handled
      await waitFor(() => {
        expect(consoleSpy).toHaveBeenCalled();
      }, { timeout: 1000 });

      expect(consoleSpy).toHaveBeenCalledWith('Failed to copy:', expect.any(Error));

      // Should still show "Copy" (not "Copied") after failure
      expect(screen.getByText('Copy')).toBeInTheDocument();

      consoleSpy.mockRestore();
    });
  });

  describe('code formatting', () => {
    it('preserves whitespace in code', () => {
      const codeWithWhitespace = `function test() {
    const x = 1;
    return x;
}`;
      render(
        <CodeBlock
          code={codeWithWhitespace}
          language="javascript"
          isDark={true}
        />
      );

      const codeElement = screen.getByText(/function test/);
      expect(codeElement).toBeInTheDocument();
      expect(codeElement.textContent).toBe(codeWithWhitespace);
    });

    it('renders code inside a pre and code element', () => {
      const { container } = render(
        <CodeBlock
          code="test code"
          language="text"
          isDark={true}
        />
      );

      const preElement = container.querySelector('pre');
      const codeElement = container.querySelector('code');

      expect(preElement).toBeInTheDocument();
      expect(codeElement).toBeInTheDocument();
      expect(codeElement?.classList.contains('font-mono')).toBe(true);
    });
  });
});
