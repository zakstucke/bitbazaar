import type React from "react";
import { useMemo } from "react";

export const ValiWrapper = ({
    params,
    validators,
    Comp,
    Comp404,
}: {
    params: object;
    validators: Record<string, (param: string) => unknown>;
    Comp: React.ComponentType;
    Comp404: React.ComponentType;
}) => {
    // biome-ignore lint/correctness/useExhaustiveDependencies: <explanation>
    const cleanedParams: object | null = useMemo(() => {
        try {
            const cleaned = {};
            for (const [key, value] of Object.entries(params)) {
                const validator = validators[key];
                if (!validator) {
                    return null;
                }
                cleaned[key] = validator(value as string);
            }
            return cleaned;
        } catch (e) {
            return null;
        }
    }, [validators, params]);

    if (cleanedParams === null) {
        return <Comp404 />;
    }

    return <Comp {...cleanedParams} />;
};
