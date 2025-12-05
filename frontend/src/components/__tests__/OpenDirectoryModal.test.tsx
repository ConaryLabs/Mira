// src/components/__tests__/OpenDirectoryModal.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { OpenDirectoryModal } from '../OpenDirectoryModal';

describe('OpenDirectoryModal', () => {
  let mockOnClose: ReturnType<typeof vi.fn<[], void>>;
  let mockOnOpen: ReturnType<typeof vi.fn<[string], Promise<boolean>>>;

  beforeEach(() => {
    mockOnClose = vi.fn<[], void>();
    mockOnOpen = vi.fn<[string], Promise<boolean>>().mockResolvedValue(true);
  });

  describe('rendering', () => {
    it('renders when isOpen is true', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      // Header text
      expect(screen.getByRole('heading', { name: 'Open Directory' })).toBeInTheDocument();
      expect(screen.getByLabelText(/Directory Path/)).toBeInTheDocument();
    });

    it('does not render when isOpen is false', () => {
      render(
        <OpenDirectoryModal
          isOpen={false}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      expect(screen.queryByRole('heading', { name: 'Open Directory' })).not.toBeInTheDocument();
    });

    it('shows opening state on submit button', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={true}
        />
      );

      expect(screen.getByText('Opening...')).toBeInTheDocument();
    });

    it('has autofocus on path input', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const pathInput = screen.getByLabelText(/Directory Path/);
      expect(pathInput).toHaveFocus();
    });
  });

  describe('form validation', () => {
    it('submit button is disabled when path is empty', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const submitButton = screen.getByRole('button', { name: /Open Directory/ });
      expect(submitButton).toBeDisabled();
    });

    it('submit button is disabled when path is only whitespace', async () => {
      const user = userEvent.setup();
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const pathInput = screen.getByLabelText(/Directory Path/);
      await user.type(pathInput, '   ');

      const submitButton = screen.getByRole('button', { name: /Open Directory/ });
      expect(submitButton).toBeDisabled();
    });

    it('submit button is enabled when path has value', async () => {
      const user = userEvent.setup();
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const pathInput = screen.getByLabelText(/Directory Path/);
      await user.type(pathInput, '/home/user/project');

      const submitButton = screen.getByRole('button', { name: /Open Directory/ });
      expect(submitButton).toBeEnabled();
    });

    it('path input is required', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const pathInput = screen.getByLabelText(/Directory Path/);
      expect(pathInput).toBeRequired();
    });
  });

  describe('user interactions', () => {
    it('closes modal when close button is clicked', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const closeButton = screen.getAllByRole('button').find(
        (btn) => btn.querySelector('svg')
      );
      fireEvent.click(closeButton!);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it('closes modal when cancel button is clicked', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const cancelButton = screen.getByRole('button', { name: /Cancel/ });
      fireEvent.click(cancelButton);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it('calls onOpen with path on submit', async () => {
      mockOnOpen.mockResolvedValue(true);
      const user = userEvent.setup();

      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const pathInput = screen.getByLabelText(/Directory Path/);
      await user.type(pathInput, '/home/user/project');

      const submitButton = screen.getByRole('button', { name: /Open Directory/ });
      await user.click(submitButton);

      expect(mockOnOpen).toHaveBeenCalledWith('/home/user/project');
    });

    it('clears form and closes modal on successful open', async () => {
      mockOnOpen.mockResolvedValue(true);
      const user = userEvent.setup();

      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const pathInput = screen.getByLabelText(/Directory Path/) as HTMLInputElement;
      await user.type(pathInput, '/home/user/project');

      const submitButton = screen.getByRole('button', { name: /Open Directory/ });
      await user.click(submitButton);

      await waitFor(() => {
        expect(mockOnClose).toHaveBeenCalledTimes(1);
      });

      expect(pathInput.value).toBe('');
    });

    it('does not close modal on failed open', async () => {
      mockOnOpen.mockResolvedValue(false);
      const user = userEvent.setup();

      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const pathInput = screen.getByLabelText(/Directory Path/);
      await user.type(pathInput, '/home/user/project');

      const submitButton = screen.getByRole('button', { name: /Open Directory/ });
      await user.click(submitButton);

      await waitFor(() => {
        expect(mockOnOpen).toHaveBeenCalled();
      });

      expect(mockOnClose).not.toHaveBeenCalled();
    });
  });

  describe('disabled state during opening', () => {
    it('disables all inputs when opening', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={true}
        />
      );

      expect(screen.getByLabelText(/Directory Path/)).toBeDisabled();
      expect(screen.getByRole('button', { name: /Opening.../ })).toBeDisabled();
      expect(screen.getByRole('button', { name: /Cancel/ })).toBeDisabled();
    });

    it('disables close button when opening', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={true}
        />
      );

      const closeButton = screen.getAllByRole('button').find(
        (btn) => btn.querySelector('svg')
      );
      expect(closeButton).toBeDisabled();
    });
  });

  describe('placeholder text', () => {
    it('shows appropriate placeholder for path input', () => {
      render(
        <OpenDirectoryModal
          isOpen={true}
          onClose={mockOnClose}
          onOpen={mockOnOpen}
          opening={false}
        />
      );

      const pathInput = screen.getByLabelText(/Directory Path/);
      expect(pathInput).toHaveAttribute('placeholder', '/home/user/my-project');
    });
  });
});
