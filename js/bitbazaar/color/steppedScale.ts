import type { ChromaStatic, Color } from "chroma-js";

interface SteppedScaleProps {
    /** Chroma instance to use, allows using a modified version to reduce bundle size downstream if needed. */
    chroma: ChromaStatic;
    /** The color to create a scale around. Expects a hex or similar that chroma can receive. */
    color: string | number | Color;
    /** The number of steps to create. Including the base color entered. */
    numberOfSteps: number;
}

/** Creates a scale around an input color with the requested number of steps.
 * E.g. 5 steps requested, it will go: darker1, darker2, input, lighter1, lighter2.
 */
export const createSteppedScale = ({
    chroma,
    color,
    numberOfSteps,
}: SteppedScaleProps): string[] => {
    const baseHex = chroma(color).hex().toLowerCase();
    const whiteHex = chroma("white").hex().toLowerCase();
    const blackHex = chroma("black").hex().toLowerCase();

    // If its white or black, just return the same for all steps:
    if (baseHex === whiteHex || baseHex === blackHex) {
        return Array(numberOfSteps).fill(baseHex);
    }

    const baseNum = Math.ceil(numberOfSteps / 2);

    const steps: string[] = [];

    // Try up to 5 times to produce values that don't end in white or black (i.e. the step size too large)
    const numAttempts = 5;
    for (let attempt = 1; attempt <= numAttempts; attempt++) {
        // Reduce the step size each attempt, to try and get a scale that doesn't hit white or black:
        const stepSize = 0.5 * (1 / attempt);
        let failed = false;
        for (let i = 1; i <= numberOfSteps; i++) {
            let derivCol: string;
            if (i < baseNum) {
                derivCol = chroma(color)
                    .darken((baseNum - i) * stepSize)
                    .hex();
            } else if (i === baseNum) {
                derivCol = baseHex;
            } else {
                derivCol = chroma(color)
                    .brighten((i - baseNum) * stepSize)
                    .hex();
            }

            if (derivCol === whiteHex || derivCol === blackHex) {
                failed = true;
            }

            steps.push(derivCol);
        }

        if (!failed) {
            break;
        }

        // Reset steps to try again:
        steps.length = 0;
    }

    return steps;
};
