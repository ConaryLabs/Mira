// src/components/__tests__/CreateProjectModal.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { CreateProjectModal } from '../CreateProjectModal';

describe('CreateProjectModal', () => {
  let mockOnClose: ReturnType<typeof vi.fn>;
  let mockOnCreate: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockOnClose = vi.fn();
    mockOnCreate = vi.fn();
  });

  describe('rendering', () => {
    it('renders when isOpen is true', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      expect(screen.getByText('Create New Project')).toBeInTheDocument();
      expect(screen.getByLabelText(/Project Name/)).toBeInTheDocument();
      expect(screen.getByLabelText(/Description/)).toBeInTheDocument();
    });

    it('does not render when isOpen is false', () => {
      render(
        <CreateProjectModal
          isOpen={false}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      expect(screen.queryByText('Create New Project')).not.toBeInTheDocument();
    });

    it('shows creating state on submit button', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={true}
        />
      );

      expect(screen.getByText('Creating...')).toBeInTheDocument();
    });

    it('has autofocus on name input', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/);
      // React's autoFocus doesn't render as an attribute, check if element is focused
      expect(nameInput).toHaveFocus();
    });
  });

  describe('form validation', () => {
    it('submit button is disabled when name is empty', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const submitButton = screen.getByRole('button', { name: /Create Project/ });
      expect(submitButton).toBeDisabled();
    });

    it('submit button is disabled when name is only whitespace', async () => {
      const user = userEvent.setup();
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/);
      await user.type(nameInput, '   ');

      const submitButton = screen.getByRole('button', { name: /Create Project/ });
      expect(submitButton).toBeDisabled();
    });

    it('submit button is enabled when name has value', async () => {
      const user = userEvent.setup();
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/);
      await user.type(nameInput, 'my-project');

      const submitButton = screen.getByRole('button', { name: /Create Project/ });
      expect(submitButton).toBeEnabled();
    });

    it('name input is required', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/);
      expect(nameInput).toBeRequired();
    });

    it('description input is not required', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const descriptionInput = screen.getByLabelText(/Description/);
      expect(descriptionInput).not.toBeRequired();
    });
  });

  describe('user interactions', () => {
    it('closes modal when close button is clicked', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const closeButton = screen.getAllByRole('button').find(
        (btn) => btn.querySelector('svg') // X icon button
      );
      fireEvent.click(closeButton!);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it('closes modal when cancel button is clicked', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const cancelButton = screen.getByRole('button', { name: /Cancel/ });
      fireEvent.click(cancelButton);

      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });

    it('calls onCreate with name and description on submit', async () => {
      mockOnCreate.mockResolvedValue(true);
      const user = userEvent.setup();

      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/);
      const descriptionInput = screen.getByLabelText(/Description/);

      await user.type(nameInput, 'my-project');
      await user.type(descriptionInput, 'A test project');

      const submitButton = screen.getByRole('button', { name: /Create Project/ });
      await user.click(submitButton);

      expect(mockOnCreate).toHaveBeenCalledWith('my-project', 'A test project');
    });

    it('calls onCreate with only name when description is empty', async () => {
      mockOnCreate.mockResolvedValue(true);
      const user = userEvent.setup();

      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/);
      await user.type(nameInput, 'my-project');

      const submitButton = screen.getByRole('button', { name: /Create Project/ });
      await user.click(submitButton);

      expect(mockOnCreate).toHaveBeenCalledWith('my-project', '');
    });

    it('clears form and closes modal on successful creation', async () => {
      mockOnCreate.mockResolvedValue(true);
      const user = userEvent.setup();

      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/) as HTMLInputElement;
      const descriptionInput = screen.getByLabelText(/Description/) as HTMLTextAreaElement;

      await user.type(nameInput, 'my-project');
      await user.type(descriptionInput, 'Description');

      const submitButton = screen.getByRole('button', { name: /Create Project/ });
      await user.click(submitButton);

      await waitFor(() => {
        expect(mockOnClose).toHaveBeenCalledTimes(1);
      });

      // Form should be cleared
      expect(nameInput.value).toBe('');
      expect(descriptionInput.value).toBe('');
    });

    it('does not close modal on failed creation', async () => {
      mockOnCreate.mockResolvedValue(false);
      const user = userEvent.setup();

      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/);
      await user.type(nameInput, 'my-project');

      const submitButton = screen.getByRole('button', { name: /Create Project/ });
      await user.click(submitButton);

      await waitFor(() => {
        expect(mockOnCreate).toHaveBeenCalled();
      });

      expect(mockOnClose).not.toHaveBeenCalled();
    });

    it('clears form when close button is clicked', async () => {
      const user = userEvent.setup();

      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/) as HTMLInputElement;
      const descriptionInput = screen.getByLabelText(/Description/) as HTMLTextAreaElement;

      await user.type(nameInput, 'my-project');
      await user.type(descriptionInput, 'Description');

      const closeButton = screen.getAllByRole('button').find(
        (btn) => btn.querySelector('svg')
      );
      fireEvent.click(closeButton!);

      // Re-render to check cleared state
      const { rerender } = render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const newNameInput = screen.getByLabelText(/Project Name/) as HTMLInputElement;
      const newDescInput = screen.getByLabelText(/Description/) as HTMLTextAreaElement;

      expect(newNameInput.value).toBe('');
      expect(newDescInput.value).toBe('');
    });
  });

  describe('disabled state during creation', () => {
    it('disables all inputs when creating', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={true}
        />
      );

      expect(screen.getByLabelText(/Project Name/)).toBeDisabled();
      expect(screen.getByLabelText(/Description/)).toBeDisabled();
      expect(screen.getByRole('button', { name: /Creating.../ })).toBeDisabled();
      expect(screen.getByRole('button', { name: /Cancel/ })).toBeDisabled();
    });

    it('disables close button when creating', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={true}
        />
      );

      const closeButton = screen.getAllByRole('button').find(
        (btn) => btn.querySelector('svg')
      );
      expect(closeButton).toBeDisabled();
    });
  });

  describe('placeholder text', () => {
    it('shows appropriate placeholder for name input', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const nameInput = screen.getByLabelText(/Project Name/);
      expect(nameInput).toHaveAttribute('placeholder', 'my-project');
    });

    it('shows appropriate placeholder for description input', () => {
      render(
        <CreateProjectModal
          isOpen={true}
          onClose={mockOnClose}
          onCreate={mockOnCreate}
          creating={false}
        />
      );

      const descriptionInput = screen.getByLabelText(/Description/);
      expect(descriptionInput).toHaveAttribute(
        'placeholder',
        'A brief description of your project...'
      );
    });
  });
});
