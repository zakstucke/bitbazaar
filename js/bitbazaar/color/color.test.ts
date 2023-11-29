import chroma from "chroma-js";
import { assert, describe, expect, it } from "vitest";

import { createSteppedScale } from ".";

const checkScale = (numSteps: number, scale: string[], source: string) => {
    const sourceIndex = Math.ceil(numSteps / 2) - 1;
    assert.equal(scale.length, numSteps);
    assert.equal(scale[sourceIndex].toUpperCase(), source.toUpperCase());
    let lastLuminance = chroma(scale[0]).luminance();
    for (let i = 1; i < scale.length; i++) {
        const luminance = chroma(scale[i]).luminance();
        assert.ok(luminance > lastLuminance);
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
            expect(scale).toEqual(Array(5).fill(output));
        });
    });
});
