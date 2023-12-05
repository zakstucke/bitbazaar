export { add } from "./add";
export { genPath } from "./genPath";

export const areTestsRunning = process.env.NODE_ENV === "test";

/** Placed in switch default, enforces compiler to check all cases in a switch are matched and this is never reached: */
export const assertNever = (neverVal: never): never => {
    throw new Error(
        // eslint-disable-next-line @typescript-eslint/restrict-template-expressions
        `Internal error. This should never have happened! Invalid neverValue: "${neverVal}".`,
    );
};
