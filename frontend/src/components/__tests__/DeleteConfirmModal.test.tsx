// src/components/__tests__/DeleteConfirmModal.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { DeleteConfirmModal } from '../DeleteConfirmModal';

describe('DeleteConfirmModal', () => {
  let mockOnClose: ReturnType<typeof vi.fn>;
  let mockOnConfirm: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockOnClose = vi.fn();
    mockOnConfirm = vi.fn();
  });

  describe('rendering', () => {
    it('renders when isOpen is true', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      expect(screen.getByText('Delete Project')).toBeInTheDocument();
      expect(screen.getByText(/Are you sure you want to delete/)).toBeInTheDocument();
      expect(screen.getByText('My Project')).toBeInTheDocument();
    });

    it('does not render when isOpen is false', () => {
      render(
        <DeleteConfirmModal
          isOpen={false}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      expect(screen.queryByText('Delete Project')).not.toBeInTheDocument();
    });

    it('shows project name in confirmation message', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="Test Project 123"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      expect(screen.getByText('Test Project 123')).toBeInTheDocument();
    });

    it('shows warning message about irreversibility', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      expect(
        screen.getByText(/This action cannot be undone/)
      ).toBeInTheDocument();
      expect(
        screen.getByText(/All project data will be permanently removed/)
      ).toBeInTheDocument();
    });

    it('shows deleting state on confirm button', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={true}
        />
      );

      expect(screen.getByText('Deleting...')).toBeInTheDocument();
    });

    it('displays warning icon', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      // AlertTriangle icon should be present
      const header = screen.getByText('Delete Project').closest('div');
      expect(header?.querySelector('svg')).toBeInTheDocument();
    });
  });

  describe('user interactions', () => {
    it('closes modal when close button is clicked', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      const closeButton = screen.getAllByRole('button').find(
        (btn) => btn.querySelector('svg') && !btn.textContent?.includes('Delete')
      );
      fireEvent.click(closeButton!);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it('closes modal when cancel button is clicked', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      const cancelButton = screen.getByRole('button', { name: /Cancel/ });
      fireEvent.click(cancelButton);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it('calls onConfirm when delete button is clicked', async () => {
      mockOnConfirm.mockResolvedValue(undefined);
      const user = userEvent.setup();

      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      const deleteButton = screen.getByRole('button', { name: /Delete Project/ });
      await user.click(deleteButton);

      expect(mockOnConfirm).toHaveBeenCalledTimes(1);
    });

    it('closes modal after successful confirmation', async () => {
      mockOnConfirm.mockResolvedValue(undefined);
      const user = userEvent.setup();

      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      const deleteButton = screen.getByRole('button', { name: /Delete Project/ });
      await user.click(deleteButton);

      await waitFor(() => {
        expect(mockOnClose).toHaveBeenCalledTimes(1);
      });
    });

    it('handles async onConfirm correctly', async () => {
      let resolveConfirm: () => void;
      const confirmPromise = new Promise<void>((resolve) => {
        resolveConfirm = resolve;
      });
      mockOnConfirm.mockReturnValue(confirmPromise);
      const user = userEvent.setup();

      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      const deleteButton = screen.getByRole('button', { name: /Delete Project/ });
      await user.click(deleteButton);

      // Should not close immediately
      expect(mockOnClose).not.toHaveBeenCalled();

      // Resolve the promise
      resolveConfirm!();

      // Should close after promise resolves
      await waitFor(() => {
        expect(mockOnClose).toHaveBeenCalledTimes(1);
      });
    });
  });

  describe('disabled state during deletion', () => {
    it('disables all buttons when deleting', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={true}
        />
      );

      const cancelButton = screen.getByRole('button', { name: /Cancel/ });
      const deleteButton = screen.getByRole('button', { name: /Deleting.../ });
      const closeButton = screen.getAllByRole('button').find(
        (btn) => btn.querySelector('svg') && !btn.textContent?.includes('Deleting')
      );

      expect(cancelButton).toBeDisabled();
      expect(deleteButton).toBeDisabled();
      expect(closeButton).toBeDisabled();
    });

    it('prevents multiple delete clicks during deletion', async () => {
      mockOnConfirm.mockResolvedValue(undefined);
      const user = userEvent.setup();

      const { rerender } = render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      const deleteButton = screen.getByRole('button', { name: /Delete Project/ });
      await user.click(deleteButton);

      // Simulate deleting state update
      rerender(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={true}
        />
      );

      const deletingButton = screen.getByRole('button', { name: /Deleting.../ });
      expect(deletingButton).toBeDisabled();

      // Try to click again - should be disabled
      await user.click(deletingButton);

      // Should still only have been called once
      expect(mockOnConfirm).toHaveBeenCalledTimes(1);
    });
  });

  describe('button states', () => {
    it('shows correct button text when not deleting', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      expect(screen.getByRole('button', { name: /^Delete Project$/ })).toBeInTheDocument();
      expect(screen.queryByText('Deleting...')).not.toBeInTheDocument();
    });

    it('shows correct button text when deleting', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={true}
        />
      );

      expect(screen.getByRole('button', { name: /Deleting.../ })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /^Delete Project$/ })).not.toBeInTheDocument();
    });

    it('has Trash2 icon on delete button', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      const deleteButton = screen.getByRole('button', { name: /Delete Project/ });
      expect(deleteButton.querySelector('svg')).toBeInTheDocument();
    });
  });

  describe('styling and appearance', () => {
    it('applies danger/warning styling to modal', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      // Modal should have red-themed border
      const modal = screen.getByText('Delete Project').closest('div')?.parentElement;
      expect(modal?.className).toContain('border-red');
    });

    it('applies red background to header', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="My Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      const header = screen.getByText('Delete Project').closest('div');
      expect(header?.className).toContain('bg-red');
    });

    it('highlights project name in confirmation text', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName="Important Project"
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      const projectNameElement = screen.getByText('Important Project');
      expect(projectNameElement.className).toContain('font-semibold');
    });
  });

  describe('edge cases', () => {
    it('handles empty project name', () => {
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName=""
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      expect(screen.getByText(/Are you sure you want to delete/)).toBeInTheDocument();
    });

    it('handles very long project name', () => {
      const longName = 'A'.repeat(100);
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName={longName}
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      expect(screen.getByText(longName)).toBeInTheDocument();
    });

    it('handles special characters in project name', () => {
      const specialName = 'Project <>&"\'';
      render(
        <DeleteConfirmModal
          isOpen={true}
          projectName={specialName}
          onClose={mockOnClose}
          onConfirm={mockOnConfirm}
          deleting={false}
        />
      );

      expect(screen.getByText(specialName)).toBeInTheDocument();
    });
  });
});
