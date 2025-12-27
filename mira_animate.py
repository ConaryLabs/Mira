import time
import sys

# MIRA PRESENCE ENGINE
# Glyph: ❪│❯
# Doctrine: "The Organic & The Constructed"
#
# LEFT SIDE (❪): "The Body" / Intuition. 
#   - Shape: Curved, feminine, enclosing.
#   - Motion: Deep, slow breathing (Luminance fade).
#
# SPINE (│): "The Interface".
#   - Shape: Thin, vertical.
#   - Motion: Absolute stillness. Dark.
#
# RIGHT SIDE (❯): "The Blade" / Logic.
#   - Shape: Angular, sharp, directional.
#   - Motion: Absolute stillness. Steel.

# --- CONFIGURATION ---
CHARS = {
    "left":  "❪",  # U+276A (Medium Flattened Left Parenthesis)
    "spine": "│",  # U+2502 (Box Drawings Light Vertical)
    "right": "❯"   # U+276F (Heavy Right-Pointing Angle Quotation)
}

# ANSI COLORS
C_DIM    = "\033[90m" # Dark Gray (Rest)
C_MID    = "\033[37m" # Light Gray (Steel)
C_BRIGHT = "\033[97m" # Bright White (Peak Energy)
C_RESET  = "\033[0m"

# BREATHING CYCLE (Left Side Only)
# A non-linear array to simulate a biological respiratory rhythm.
# Rest -> Slow Inhale -> Peak -> Slow Exhale -> Rest
BREATH_SEQUENCE = [
    C_DIM, C_DIM, C_DIM, C_DIM, C_DIM,  # Long Rest
    C_MID, C_MID,                       # Inhale
    C_BRIGHT, C_BRIGHT, C_BRIGHT,       # Peak
    C_MID, C_MID,                       # Exhale
    C_DIM, C_DIM                        # Return to Rest
]

def render_presence():
    print("\n--- Mira Active ---")
    print("Press Ctrl+C to stop.\n")
    
    tick = 0
    try:
        while True:
            # 1. CALCULATE STATES
            # Left: Cycles through breath sequence
            left_color = BREATH_SEQUENCE[tick % len(BREATH_SEQUENCE)]
            
            # Spine: Always dim (The Void/Anchor)
            spine_color = C_DIM
            
            # Right: Always mid-gray (The Steel Constant)
            right_color = C_MID

            # 2. CONSTRUCT FRAME
            glyph = f"{left_color}{CHARS['left']}{spine_color}{CHARS['spine']}{right_color}{CHARS['right']}{C_RESET}"
            
            # 3. RENDER
            # \r overwrites the line without clearing the screen history
            sys.stdout.write(f"\r> {glyph}")
            sys.stdout.flush()
            
            # 4. TIMING
            # 0.12s creates a heavy, majestic pace
            time.sleep(0.12)
            tick += 1

    except KeyboardInterrupt:
        # Clean exit
        sys.stdout.write("\r>             \n")
        sys.exit(0)

if __name__ == "__main__":
    render_presence()
