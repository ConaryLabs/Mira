// src/components/modals/ImageGenerationModal.tsx
// PHASE 2: Image Generation Modal for tool invocation
// Allows users to generate images with customizable options

import React, { useState, useRef, useEffect } from 'react';
import { X, Image, Palette, Maximize, Sparkles } from 'lucide-react';
import clsx from 'clsx';

interface ImageGenerationModalProps {
  onSubmit: (prompt: string, options?: ImageGenerationOptions) => void;
  onClose: () => void;
  isDark: boolean;
}

interface ImageGenerationOptions {
  size?: '1024x1024' | '1024x1792' | '1792x1024';
  style?: 'vivid' | 'natural';
  quality?: 'standard' | 'hd';
  n?: number; // number of images to generate
}

export const ImageGenerationModal: React.FC<ImageGenerationModalProps> = ({
  onSubmit,
  onClose,
  isDark
}) => {
  const [prompt, setPrompt] = useState('');
  const [options, setOptions] = useState<ImageGenerationOptions>({
    size: '1024x1024',
    style: 'vivid',
    quality: 'standard',
    n: 1
  });
  const [showAdvanced, setShowAdvanced] = useState(false);
  
  const modalRef = useRef<HTMLDivElement>(null);
  const promptInputRef = useRef<HTMLTextAreaElement>(null);

  // Focus prompt input when modal opens
  useEffect(() => {
    promptInputRef.current?.focus();
  }, []);

  // Handle outside click to close
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (modalRef.current && !modalRef.current.contains(event.target as Node)) {
        onClose();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [onClose]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!prompt.trim()) return;

    onSubmit(prompt.trim(), options);
  };

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      onClose();
    }
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      handleSubmit(e);
    }
  };

  const examplePrompts = [
    "A futuristic cityscape at sunset with flying cars",
    "A cozy cabin in a snowy forest with warm lights",
    "An abstract painting with vibrant colors and geometric shapes",
    "A minimalist logo design for a tech startup",
    "A photorealistic portrait of a wise elderly person",
    "A fantasy landscape with floating islands and waterfalls"
  ];

  return (
    <div className="fixed inset-0 bg-black/50 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div 
        ref={modalRef}
        className={clsx(
          'w-full max-w-lg rounded-xl shadow-2xl',
          isDark ? 'bg-gray-800 text-white' : 'bg-white text-gray-900'
        )}
        onKeyDown={handleKeyPress}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-gray-200 dark:border-gray-700">
          <div className="flex items-center gap-2">
            <Image className="w-5 h-5" />
            <h2 className="text-lg font-semibold">Generate Image</h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="p-6">
          <div className="space-y-4">
            {/* Prompt Input */}
            <div>
              <label className="block text-sm font-medium mb-2">
                Image Description
              </label>
              <textarea
                ref={promptInputRef}
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                placeholder="Describe the image you want to generate in detail..."
                rows={4}
                className={clsx(
                  'w-full px-4 py-3 rounded-lg border transition-colors resize-none',
                  'focus:outline-none focus:ring-2 focus:ring-purple-500 focus:border-transparent',
                  isDark
                    ? 'bg-gray-700 border-gray-600 text-white placeholder-gray-400'
                    : 'bg-white border-gray-300 text-gray-900 placeholder-gray-500'
                )}
                required
              />
              <div className="flex justify-between items-center mt-1">
                <p className="text-xs text-gray-500">
                  Press Cmd/Ctrl + Enter to generate
                </p>
                <span className={clsx(
                  'text-xs',
                  prompt.length > 1000 ? 'text-red-500' : 'text-gray-500'
                )}>
                  {prompt.length}/1000
                </span>
              </div>
            </div>

            {/* Example Prompts */}
            <div>
              <label className="block text-sm font-medium mb-2">
                Example Prompts
              </label>
              <div className="grid grid-cols-1 gap-1 max-h-32 overflow-y-auto">
                {examplePrompts.map((example, index) => (
                  <button
                    key={index}
                    type="button"
                    onClick={() => setPrompt(example)}
                    className={clsx(
                      'text-left text-xs p-2 rounded border transition-colors',
                      isDark
                        ? 'border-gray-600 hover:bg-gray-700 text-gray-300'
                        : 'border-gray-200 hover:bg-gray-50 text-gray-600'
                    )}
                  >
                    {example}
                  </button>
                ))}
              </div>
            </div>

            {/* Advanced Options Toggle */}
            <button
              type="button"
              onClick={() => setShowAdvanced(!showAdvanced)}
              className="flex items-center gap-2 text-sm text-purple-600 dark:text-purple-400 hover:underline"
            >
              <Sparkles className="w-4 h-4" />
              Advanced Options
            </button>

            {/* Advanced Options */}
            {showAdvanced && (
              <div className="space-y-4 pt-4 border-t border-gray-200 dark:border-gray-700">
                {/* Size Selection */}
                <div>
                  <label className="block text-sm font-medium mb-2">
                    <Maximize className="inline w-4 h-4 mr-1" />
                    Image Size
                  </label>
                  <div className="grid grid-cols-3 gap-2">
                    {[
                      { value: '1024x1024', label: 'Square\n1:1', desc: '1024×1024' },
                      { value: '1024x1792', label: 'Portrait\n9:16', desc: '1024×1792' },
                      { value: '1792x1024', label: 'Landscape\n16:9', desc: '1792×1024' }
                    ].map((size) => (
                      <button
                        key={size.value}
                        type="button"
                        onClick={() => setOptions(prev => ({ ...prev, size: size.value as any }))}
                        className={clsx(
                          'p-3 rounded-lg border-2 transition-colors text-center text-sm',
                          options.size === size.value
                            ? 'border-purple-500 bg-purple-50 dark:bg-purple-900/20 text-purple-700 dark:text-purple-300'
                            : isDark
                              ? 'border-gray-600 hover:border-gray-500 text-gray-300'
                              : 'border-gray-300 hover:border-gray-400 text-gray-700'
                        )}
                      >
                        <div className="whitespace-pre-line font-medium">{size.label}</div>
                        <div className="text-xs opacity-75">{size.desc}</div>
                      </button>
                    ))}
                  </div>
                </div>

                {/* Style Selection */}
                <div>
                  <label className="block text-sm font-medium mb-2">
                    <Palette className="inline w-4 h-4 mr-1" />
                    Art Style
                  </label>
                  <div className="grid grid-cols-2 gap-2">
                    {[
                      { value: 'vivid', label: 'Vivid', desc: 'More dramatic, vibrant colors' },
                      { value: 'natural', label: 'Natural', desc: 'More realistic, subdued' }
                    ].map((style) => (
                      <button
                        key={style.value}
                        type="button"
                        onClick={() => setOptions(prev => ({ ...prev, style: style.value as any }))}
                        className={clsx(
                          'p-3 rounded-lg border-2 transition-colors text-left',
                          options.style === style.value
                            ? 'border-purple-500 bg-purple-50 dark:bg-purple-900/20 text-purple-700 dark:text-purple-300'
                            : isDark
                              ? 'border-gray-600 hover:border-gray-500 text-gray-300'
                              : 'border-gray-300 hover:border-gray-400 text-gray-700'
                        )}
                      >
                        <div className="font-medium">{style.label}</div>
                        <div className="text-xs opacity-75">{style.desc}</div>
                      </button>
                    ))}
                  </div>
                </div>

                {/* Quality Selection */}
                <div>
                  <label className="block text-sm font-medium mb-2">
                    Image Quality
                  </label>
                  <div className="grid grid-cols-2 gap-2">
                    {[
                      { value: 'standard', label: 'Standard', desc: 'Faster generation' },
                      { value: 'hd', label: 'HD', desc: 'Higher quality, slower' }
                    ].map((quality) => (
                      <button
                        key={quality.value}
                        type="button"
                        onClick={() => setOptions(prev => ({ ...prev, quality: quality.value as any }))}
                        className={clsx(
                          'p-3 rounded-lg border-2 transition-colors text-left',
                          options.quality === quality.value
                            ? 'border-purple-500 bg-purple-50 dark:bg-purple-900/20 text-purple-700 dark:text-purple-300'
                            : isDark
                              ? 'border-gray-600 hover:border-gray-500 text-gray-300'
                              : 'border-gray-300 hover:border-gray-400 text-gray-700'
                        )}
                      >
                        <div className="font-medium">{quality.label}</div>
                        <div className="text-xs opacity-75">{quality.desc}</div>
                      </button>
                    ))}
                  </div>
                </div>

                {/* Number of Images */}
                <div>
                  <label className="block text-sm font-medium mb-2">
                    Number of Images
                  </label>
                  <select
                    value={options.n}
                    onChange={(e) => setOptions(prev => ({ ...prev, n: parseInt(e.target.value) }))}
                    className={clsx(
                      'w-full px-3 py-2 rounded-lg border transition-colors',
                      isDark
                        ? 'bg-gray-700 border-gray-600 text-white'
                        : 'bg-white border-gray-300 text-gray-900'
                    )}
                  >
                    <option value={1}>1 image</option>
                    <option value={2}>2 images</option>
                    <option value={3}>3 images</option>
                    <option value={4}>4 images</option>
                  </select>
                </div>
              </div>
            )}
          </div>

          {/* Actions */}
          <div className="flex gap-3 pt-6">
            <button
              type="button"
              onClick={onClose}
              className={clsx(
                'flex-1 px-4 py-2 rounded-lg border transition-colors',
                isDark
                  ? 'border-gray-600 text-gray-300 hover:bg-gray-700'
                  : 'border-gray-300 text-gray-700 hover:bg-gray-50'
              )}
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!prompt.trim() || prompt.length > 1000}
              className={clsx(
                'flex-1 px-4 py-2 rounded-lg transition-colors font-medium flex items-center justify-center gap-2',
                !prompt.trim() || prompt.length > 1000
                  ? 'bg-gray-400 text-gray-600 cursor-not-allowed'
                  : 'bg-purple-600 text-white hover:bg-purple-700'
              )}
            >
              <Image className="w-4 h-4" />
              Generate Image{options.n && options.n > 1 ? `s (${options.n})` : ''}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};
