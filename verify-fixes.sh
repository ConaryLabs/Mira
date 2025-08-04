#!/bin/bash

echo "🔍 VERIFYING MIRA'S PERSONALITY FIXES"
echo "======================================"
echo ""

# Check if ChatService is using the persona prompt
echo "1. Checking if ChatService.rs has the persona fix:"
echo "---------------------------------------------------"
grep -n "system_prompt.push_str(persona.prompt())" src/services/chat.rs
if [ $? -eq 0 ]; then
    echo "✅ ChatService IS using persona.prompt()"
else
    echo "❌ ChatService is NOT using persona.prompt() - fix not applied!"
fi

echo ""

# Check if it's using the generic prompt
echo "2. Checking for the old generic prompt:"
echo "----------------------------------------"
grep -n '"You are Mira. Be witty, warm, and real"' src/services/chat.rs
if [ $? -eq 0 ]; then
    echo "❌ PROBLEM: ChatService still has the generic prompt!"
    echo "   This is overriding Mira's real personality!"
else
    echo "✅ Generic prompt removed"
fi

echo ""

# Check HybridService
echo "3. Checking if HybridService has the fix:"
echo "------------------------------------------"
grep -n "messages.insert(0, system_message)" src/services/hybrid.rs
if [ $? -eq 0 ]; then
    echo "✅ HybridService has the persona fix"
else
    echo "⚠️  HybridService might not have the fix"
fi

echo ""

# Check what DEFAULT_PERSONA_PROMPT contains
echo "4. First few lines of DEFAULT_PERSONA_PROMPT:"
echo "----------------------------------------------"
head -n 10 src/persona/default.rs | grep -A 5 "You are Mira"

echo ""
echo ""
echo "DIAGNOSIS:"
echo "----------"
echo "If you see the generic prompt still in ChatService, that's the problem!"
echo "The Default persona has Mira's FULL personality, but it's being overridden."
echo ""
echo "To fix: Make sure ChatService uses persona.prompt() instead of a hardcoded prompt."
