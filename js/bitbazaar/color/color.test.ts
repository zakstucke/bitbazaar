import { describe, expect, it } from "bun:test";
import chroma from "chroma-js";

import { createSteppedScale } from ".";

const checkScale = (numSteps: number, scale: string[], source: string) => {
    const sourceIndex = Math.ceil(numSteps / 2) - 1;
    expect(scale.length).toBe(numSteps);
    expect(scale[sourceIndex].toUpperCase()).toBe(source.toUpperCase());
    let lastLuminance = chroma(scale[0]).luminance();
    for (let i = 1; i < scale.length; i++) {
        const luminance = chroma(scale[i]).luminance();
        expect(luminance).toBeGreaterThan(lastLuminance);
        lastLuminance = luminance;
    }
};

describe("Color", () => {
    describe("steppedScale", () => {
        it("Intended usage", () => {
            const red = "#ff0000";
            const scale = createSteppedScale({
                chroma,
                color: red,
                numberOfSteps: 5,
            });
            checkScale(5, scale, red);
        });

        it("Retries work to produce smaller scales if need be", () => {
            const redCloseToWhite = "#F6E7E4";
            const scale = createSteppedScale({
                chroma,
                color: redCloseToWhite,
                numberOfSteps: 5,
            });
            checkScale(5, scale, redCloseToWhite);
        });

        it.each([
            ["white", "#ffffff"],
            ["black", "#000000"],
            ["#ffffff", "#ffffff"],
            ["#000000", "#000000"],
        ])("static colors: %i should output %i across all steps", (input, output) => {
            const scale = createSteppedScale({
                chroma,
                color: input,
                numberOfSteps: 5,
            });
            expect(scale).toEqual([output, output, output, output, output]);
        });
    });
});
